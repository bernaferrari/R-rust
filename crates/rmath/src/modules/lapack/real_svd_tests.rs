//! Direct safety contracts for the real SVD embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_svd;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{INTEGER, REAL, SET_ATTRIB, SETTAG, VECTOR_ELT};
use crate::sexp::constructors::{Rf_allocVector3, Rf_cons, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::protect::protect;
use crate::sexp::session::RSession;

unsafe fn make_input(dimensions: &[i32], values: &[f64]) -> SEXP {
    unsafe {
        let data = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
        let _data_guard = protect(data);
        let dims = Rf_allocVector3(SEXPTYPE::INTSXP, dimensions.len() as R_xlen_t);
        let _dims_guard = protect(dims);
        for (i, value) in dimensions.iter().enumerate() {
            *INTEGER(dims).add(i) = *value;
        }
        let attrs = Rf_cons(dims, R_NilValue());
        SETTAG(attrs, R_DimSymbol());
        SET_ATTRIB(data, attrs);
        for (i, value) in values.iter().enumerate() {
            *REAL(data).add(i) = *value;
        }
        data
    }
}

unsafe fn make_jobu() -> SEXP {
    unsafe { Rf_mkString(b"N\0".as_ptr() as *const std::os::raw::c_char) }
}

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid matrix should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

unsafe fn svd_args(n: i32, p: i32, values: &[f64]) -> (SEXP, SEXP, SEXP, SEXP, SEXP) {
    unsafe {
        let jobu = make_jobu();
        let x = make_input(&[n, p], values);
        let min_np = if n < p { n } else { p };
        let s = Rf_allocVector3(SEXPTYPE::REALSXP, min_np as R_xlen_t);
        if n == 0 || p == 0 {
            let u = make_input(&[0, 0], &[]);
            let vt = make_input(&[0, 0], &[]);
            return (jobu, x, s, u, vt);
        }
        let u = make_input(&[n, n], &vec![0.0; (n * n) as usize]);
        let vt = make_input(&[p, p], &vec![0.0; (p * p) as usize]);
        (jobu, x, s, u, vt)
    }
}

#[test]
fn real_svd_accepts_zero_dimensional_shapes() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        for dims in [[0, 3], [3, 0]] {
            let (jobu, x, s, u, vt) = svd_args(dims[0], dims[1], &[]);
            let _j = protect(jobu);
            let _x = protect(x);
            let _s = protect(s);
            let _u = protect(u);
            let _vt = protect(vt);
            La_svd(jobu, x, s, u, vt);
        }
    });
}

#[test]
fn real_svd_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let jobu = make_jobu();
        let _j = protect(jobu);
        let valid_s = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _s = protect(valid_s);
        let valid_u = make_input(&[2, 2], &[0.0, 0.0, 0.0, 0.0]);
        let _u = protect(valid_u);
        let valid_vt = make_input(&[2, 2], &[0.0, 0.0, 0.0, 0.0]);
        let _vt = protect(valid_vt);
        for (dims, values) in [
            (vec![2], vec![1.0, 0.0]),
            (vec![2, 2], vec![1.0, 0.0, 0.0]),
            (vec![-1, 2], vec![1.0, 0.0]),
            (vec![i32::MAX, i32::MAX], vec![1.0]),
        ] {
            let x = make_input(&dims, &values);
            let _guard = protect(x);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_svd(jobu, x, valid_s, valid_u, valid_vt);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }
        let x = make_input(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        let _x = protect(x);
        let short_s = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _ss = protect(short_s);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd(jobu, x, short_s, valid_u, valid_vt);
        })));
        assert!(
            message.contains("matrix") || message.contains("dimension"),
            "{message}"
        );
        let x = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _xc = protect(x);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd(jobu, x, valid_s, valid_u, valid_vt);
        })));
        assert!(message.contains("numeric matrix"), "{message}");
        let x = make_input(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        let _x2 = protect(x);
        let jobu = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _jb = protect(jobu);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd(jobu, x, valid_s, valid_u, valid_vt);
        })));
        assert!(message.contains("character string"), "{message}");
    });
}

#[test]
fn real_svd_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (jobu, x, s, u, vt) =
        session.with_active(|| unsafe { svd_args(2, 2, &[2.0, 0.0, 0.0, 2.0]) });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _j = protect(jobu);
        let _x = protect(x);
        let _s = protect(s);
        let _u = protect(u);
        let _vt = protect(vt);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd(jobu, x, s, u, vt);
        })));
        assert!(
            message.contains("native SVD workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _j = protect(jobu);
        let _x = protect(x);
        let _s = protect(s);
        let _u = protect(u);
        let _vt = protect(vt);
        let ans = La_svd(jobu, x, s, u, vt);
        let _ans = protect(ans);
        let d = VECTOR_ELT(ans, 0);
        assert!((*REAL(d) - 2.0).abs() < 1e-12);
        assert!((*REAL(d).add(1) - 2.0).abs() < 1e-12);
    });
}

#[test]
fn real_svd_admission_rejects_short_vector_outputs_before_workspace() {
    use crate::sexp::accessors::{CHAR, STRING_ELT};
    use crate::sexp::instance::with_current_instance;
    use crate::sexp::memory::with_arena_in;

    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        for (job, n, p, ud, vd, short) in [
            (b"S\0", 3, 2, [3, 1], [2, 2], Some("u")),
            (b"S\0", 3, 2, [2, 2], [2, 2], Some("u")),
            (b"S\0", 2, 3, [2, 2], [1, 3], Some("vt")),
            (b"S\0", 2, 3, [2, 2], [2, 2], Some("vt")),
            (b"A\0", 3, 2, [3, 2], [2, 2], Some("u")),
            (b"A\0", 2, 3, [2, 2], [2, 3], Some("vt")),
            // Padded strides/columns are valid; job N ignores both outputs.
            (b"S\0", 3, 2, [4, 3], [3, 3], None),
            (b"A\0", 3, 2, [4, 4], [3, 3], None),
            (b"N\0", 3, 2, [0, 0], [0, 0], None),
            (b"O\0", 3, 2, [0, 0], [2, 2], None),
            (b"o\0", 2, 3, [2, 2], [0, 0], None),
            (b"O\0", 2, 2, [0, 0], [2, 2], None),
            (b"O\0", 3, 2, [0, 0], [1, 2], Some("vt")),
            (b"O\0", 2, 3, [1, 2], [0, 0], Some("u")),
            (b"X\0", 3, 2, [0, 0], [0, 0], Some("job")),
            (b"\0\0", 3, 2, [0, 0], [0, 0], Some("job")),
        ] {
            let jobu = Rf_mkString(job.as_ptr().cast());
            let _j = protect(jobu);
            let x = make_input(&[n, p], &vec![1.; (n * p) as usize]);
            let _x = protect(x);
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, n.min(p) as R_xlen_t);
            let _s = protect(s);
            let u = make_input(&ud, &vec![0.; (ud[0] * ud[1]) as usize]);
            let _u = protect(u);
            let v = make_input(&vd, &vec![0.; (vd[0] * vd[1]) as usize]);
            let _v = protect(v);
            // Warm lazy symbol/character projections before setting the budget.
            CHAR(STRING_ELT(jobu, 0));
            for matrix in [x, u, v] {
                crate::attrib_core::getAttrib(matrix, R_DimSymbol());
            }
            // Admit the five adapter protection slots before isolating its
            // scratch budget; this test targets output bounds, not root growth.
            drop([
                protect(jobu),
                protect(x),
                protect(s),
                protect(u),
                protect(v),
            ]);
            let held = with_current_instance(|instance| {
                with_arena_in(instance, |arena| {
                    let held = arena.workspace_bytes();
                    arena.set_budget(ArenaBudget::new(1, 0));
                    held
                })
            })
            .unwrap();
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_svd(jobu, x, s, u, v);
            })));
            if short == Some("job") {
                assert_eq!(message, "error code from Lapack routine 'dgesdd'");
            } else if let Some(output) = short {
                assert!(
                    message.contains(&format!("'{output}' is too small")),
                    "{message}"
                );
            } else {
                assert!(
                    message.contains("native SVD workspace exceeds resource limit"),
                    "{message}"
                );
            }
            // Even a regression that removes validation cannot reach unsafe
            // backend writes: this fixture denies all caller scratch first.
            with_current_instance(|instance| {
                with_arena_in(instance, |arena| {
                    assert_eq!(arena.workspace_bytes(), held);
                    arena.set_budget(ArenaBudget::unlimited());
                })
            });
        }
    });
}

#[test]
#[cfg(all(feature = "rust-backend", not(miri)))]
fn real_svd_backend_modes_rectangular_reconstruction_and_invalid_jobs() {
    use super::backend::dgesdd_;
    for (m, n) in [(3i32, 2i32), (2, 3), (2, 2)] {
        let (rows, cols) = (m as usize, n as usize);
        let k = rows.min(cols);
        for job in [b'A', b'S', b'O', b'n'] {
            let mut a = vec![0.; rows * cols];
            a[0] = 3.;
            a[1 + rows] = 2.;
            let original = a.clone();
            // Generous backing on both sides keeps this regression safe even
            // if O dispatch regresses to writing thin outputs to both sides.
            let mut u = vec![7.; rows * rows];
            let mut vt = vec![7.; cols * cols];
            let mut s = vec![0.; k];
            let mut iwork = vec![0; 8 * k];
            let mut query = 0.;
            let mut info = 99;
            unsafe {
                dgesdd_(
                    &job,
                    &m,
                    &n,
                    a.as_mut_ptr(),
                    &m,
                    s.as_mut_ptr(),
                    u.as_mut_ptr(),
                    &m,
                    vt.as_mut_ptr(),
                    &n,
                    &mut query,
                    &-1,
                    iwork.as_mut_ptr(),
                    &mut info,
                );
            }
            assert_eq!(info, 0);
            let lwork = query as i32;
            let mut work = vec![0.; lwork as usize];
            unsafe {
                dgesdd_(
                    &job,
                    &m,
                    &n,
                    a.as_mut_ptr(),
                    &m,
                    s.as_mut_ptr(),
                    u.as_mut_ptr(),
                    &m,
                    vt.as_mut_ptr(),
                    &n,
                    work.as_mut_ptr(),
                    &lwork,
                    iwork.as_mut_ptr(),
                    &mut info,
                );
            }
            assert_eq!(info, 0);
            assert!((s[0] - 3.).abs() < 1e-12 && (s[1] - 2.).abs() < 1e-12);
            if job == b'n' {
                assert!(u.iter().chain(&vt).all(|z| *z == 7.));
            } else {
                if job == b'O' {
                    let ignored = if rows >= cols { &u } else { &vt };
                    assert!(ignored.iter().all(|z| *z == 7.));
                }
                for j in 0..cols {
                    for i in 0..rows {
                        let sum: f64 = (0..k)
                            .map(|h| {
                                let left = if job == b'O' && rows >= cols {
                                    a[i + h * rows]
                                } else {
                                    u[i + h * rows]
                                };
                                let right = if job == b'O' && rows < cols {
                                    a[h + j * rows]
                                } else {
                                    vt[h + j * cols]
                                };
                                left * s[h] * right
                            })
                            .sum();
                        assert!((sum - original[i + j * rows]).abs() < 1e-12);
                    }
                }
            }
            for invalid in [b'X', 0] {
                let before = a.clone();
                unsafe {
                    dgesdd_(
                        &invalid,
                        &m,
                        &n,
                        a.as_mut_ptr(),
                        &m,
                        s.as_mut_ptr(),
                        u.as_mut_ptr(),
                        &m,
                        vt.as_mut_ptr(),
                        &n,
                        &mut query,
                        &-1,
                        iwork.as_mut_ptr(),
                        &mut info,
                    );
                }
                assert_eq!(info, -1);
                assert_eq!(a, before);
            }
        }
    }
}

#[test]
#[cfg(feature = "rust-backend")]
fn real_svd_admission_rectangular_workspace_stages_and_retry() {
    use crate::sexp::accessors::{CHAR, STRING_ELT};
    use crate::sexp::instance::with_current_instance;
    use crate::sexp::memory::with_arena_in;
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let (ej, ex, es, eu, ev) = svd_args(0, 0, &[]);
        let _guards = [
            protect(ej),
            protect(ex),
            protect(es),
            protect(eu),
            protect(ev),
        ];
        let warm = La_svd(ej, ex, es, eu, ev);
        let _warm = protect(warm);
        for (n, p, job) in [(3, 2, b"O\0"), (2, 3, b"O\0"), (2, 2, b"N\0")] {
            let jobu = Rf_mkString(job.as_ptr().cast());
            let _j = protect(jobu);
            let mut values = vec![0.; (n * p) as usize];
            values[0] = 3.;
            values[1 + n as usize] = 2.;
            let x = make_input(&[n, p], &values);
            let _x = protect(x);
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            let _s = protect(s);
            let ud = if n >= p || job[0] == b'N' {
                [1, 1]
            } else {
                [n, n]
            };
            let vd = if n >= p && job[0] != b'N' {
                [p, p]
            } else {
                [1, 1]
            };
            let u = make_input(&ud, &vec![7.; (ud[0] * ud[1]) as usize]);
            let _u = protect(u);
            let v = make_input(&vd, &vec![7.; (vd[0] * vd[1]) as usize]);
            let _v = protect(v);
            CHAR(STRING_ELT(jobu, 0));
            for matrix in [x, u, v] {
                crate::attrib_core::getAttrib(matrix, R_DimSymbol());
            }
            drop([
                protect(jobu),
                protect(x),
                protect(s),
                protect(u),
                protect(v),
            ]);
            let (resident, held) = with_current_instance(|instance| {
                with_arena_in(instance, |arena| {
                    (arena.total_bytes_allocated(), arena.workspace_bytes())
                })
            })
            .unwrap();
            // x copy plus 8*k integer work entries; Rust query is 36 doubles here.
            let initial = values.len() * size_of::<f64>() + 16 * size_of::<i32>();
            let queried = 36 * size_of::<f64>();
            for allowance in [initial - 1, initial + queried - 1] {
                *REAL(s) = -7.;
                *REAL(s).add(1) = -8.;
                with_current_instance(|instance| {
                    with_arena_in(instance, |arena| {
                        arena.set_budget(ArenaBudget::new(resident + held + allowance, 0))
                    })
                });
                let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                    La_svd(jobu, x, s, u, v);
                })));
                assert!(
                    message.contains("native SVD workspace exceeds resource limit"),
                    "{message}"
                );
                assert_eq!(std::slice::from_raw_parts(REAL(s), 2), &[-7., -8.]);
                with_current_instance(|instance| {
                    with_arena_in(instance, |arena| {
                        assert_eq!(arena.workspace_bytes(), held);
                        arena.set_budget(ArenaBudget::unlimited());
                    })
                });
            }
            // Native covers the numerical retry; Miri exercises both admission
            // stages without entering faer's unsupported NEON reduction.
            #[cfg(not(miri))]
            {
                let answer = La_svd(jobu, x, s, u, v);
                let _answer = protect(answer);
                let d = REAL(VECTOR_ELT(answer, 0));
                assert!((*d - 3.).abs() < 1e-12 && (*d.add(1) - 2.).abs() < 1e-12);
                let ignored = if n >= p { u } else { v };
                assert_eq!(*REAL(ignored), 7.);
                assert_eq!(std::slice::from_raw_parts(REAL(x), values.len()), &values);
            }
            with_current_instance(|instance| {
                with_arena_in(instance, |arena| assert_eq!(arena.workspace_bytes(), held))
            });
        }
    });
}
