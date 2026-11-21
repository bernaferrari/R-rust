//! Direct safety contracts for the complex SVD embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_svd_cmplx;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{COMPLEX, INTEGER, REAL, SET_ATTRIB, SETTAG, VECTOR_ELT};
use crate::sexp::constructors::{Rf_allocVector3, Rf_cons, Rf_mkString};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::protect::protect;
use crate::sexp::session::RSession;

unsafe fn make_input(dimensions: &[i32], values: &[(f64, f64)]) -> SEXP {
    unsafe {
        let data = Rf_allocVector3(SEXPTYPE::CPLXSXP, values.len() as R_xlen_t);
        let _data_guard = protect(data);
        let dims = Rf_allocVector3(SEXPTYPE::INTSXP, dimensions.len() as R_xlen_t);
        let _dims_guard = protect(dims);
        for (i, value) in dimensions.iter().enumerate() {
            *INTEGER(dims).add(i) = *value;
        }
        let attrs = Rf_cons(dims, R_NilValue());
        SETTAG(attrs, R_DimSymbol());
        SET_ATTRIB(data, attrs);
        for (i, (re, im)) in values.iter().enumerate() {
            (*COMPLEX(data).add(i)).r = *re;
            (*COMPLEX(data).add(i)).i = *im;
        }
        data
    }
}

unsafe fn make_jobu() -> SEXP {
    unsafe { crate::sexp::constructors::mk_string_str("N") }
}

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid matrix should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

unsafe fn svd_args(n: i32, p: i32, values: &[(f64, f64)]) -> (SEXP, SEXP, SEXP, SEXP, SEXP) {
    unsafe {
        let jobu = make_jobu();
        let x = make_input(&[n, p], values);
        let min_np = if n < p { n } else { p };
        let s = Rf_allocVector3(SEXPTYPE::REALSXP, min_np as R_xlen_t);
        if n == 0 || p == 0 {
            let u = make_input(&[0, 0], &[]);
            let v = make_input(&[0, 0], &[]);
            return (jobu, x, s, u, v);
        }
        let u = make_input(&[n, n], &vec![(0.0, 0.0); (n * n) as usize]);
        let v = make_input(&[p, p], &vec![(0.0, 0.0); (p * p) as usize]);
        (jobu, x, s, u, v)
    }
}

#[test]
fn complex_svd_accepts_zero_dimensional_shapes() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        for dims in [[0, 3], [3, 0]] {
            let (jobu, x, s, u, v) = svd_args(dims[0], dims[1], &[]);
            let _j = protect(jobu);
            let _x = protect(x);
            let _s = protect(s);
            let _u = protect(u);
            let _v = protect(v);
            La_svd_cmplx(jobu, x, s, u, v);
        }
    });
}

#[test]
fn complex_svd_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let jobu = make_jobu();
        let _j = protect(jobu);
        let valid_s = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
        let _s = protect(valid_s);
        let valid_u = make_input(&[2, 2], &[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        let _u = protect(valid_u);
        let valid_v = make_input(&[2, 2], &[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        let _v = protect(valid_v);
        for (dims, values) in [
            (vec![2], vec![(1.0, 0.0), (0.0, 0.0)]),
            (vec![2, 2], vec![(1.0, 0.0), (0.0, 0.0), (0.0, 0.0)]),
            (vec![-1, 2], vec![(1.0, 0.0), (0.0, 0.0)]),
            (vec![i32::MAX, i32::MAX], vec![(1.0, 0.0)]),
        ] {
            let x = make_input(&dims, &values);
            let _guard = protect(x);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_svd_cmplx(jobu, x, valid_s, valid_u, valid_v);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }
        let x = make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]);
        let _x = protect(x);
        let short_s = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _ss = protect(short_s);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd_cmplx(jobu, x, short_s, valid_u, valid_v);
        })));
        assert!(
            message.contains("matrix") || message.contains("dimension"),
            "{message}"
        );
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _xc = protect(x);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd_cmplx(jobu, x, valid_s, valid_u, valid_v);
        })));
        assert!(message.contains("complex matrix"), "{message}");
        let x = make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]);
        let _x2 = protect(x);
        let jobu = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _jb = protect(jobu);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd_cmplx(jobu, x, valid_s, valid_u, valid_v);
        })));
        assert!(message.contains("character string"), "{message}");
    });
}

#[test]
fn complex_svd_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (jobu, x, s, u, v) = session.with_active(|| unsafe {
        svd_args(2, 2, &[(2.0, 0.0), (0.0, 0.0), (0.0, 0.0), (2.0, 0.0)])
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _j = protect(jobu);
        let _x = protect(x);
        let _s = protect(s);
        let _u = protect(u);
        let _v = protect(v);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_svd_cmplx(jobu, x, s, u, v);
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
        let _v = protect(v);
        let ans = La_svd_cmplx(jobu, x, s, u, v);
        let _ans = protect(ans);
        let d = VECTOR_ELT(ans, 0);
        assert!((*REAL(d) - 2.0).abs() < 1e-12);
        assert!((*REAL(d).add(1) - 2.0).abs() < 1e-12);
    });
}

#[test]
fn complex_svd_admission_rejects_short_vector_outputs_before_workspace() {
    use crate::sexp::accessors::{CHAR, STRING_ELT};
    use crate::sexp::instance::with_current_instance;
    use crate::sexp::memory::with_arena_in;

    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        for (job, n, p, ud, vd, short) in [
            (b"S\0", 3, 2, [3, 1], [2, 2], Some("u")),
            (b"S\0", 3, 2, [2, 2], [2, 2], Some("u")),
            (b"S\0", 2, 3, [2, 2], [1, 3], Some("v")),
            (b"S\0", 2, 3, [2, 2], [2, 2], Some("v")),
            (b"A\0", 3, 2, [3, 2], [2, 2], Some("u")),
            (b"A\0", 2, 3, [2, 2], [2, 3], Some("v")),
            // Padded strides/columns are valid; job N ignores both outputs.
            (b"S\0", 3, 2, [4, 3], [3, 3], None),
            (b"A\0", 3, 2, [4, 4], [3, 3], None),
            (b"N\0", 3, 2, [0, 0], [0, 0], None),
            (b"O\0", 3, 2, [0, 0], [2, 2], None),
            (b"o\0", 2, 3, [2, 2], [0, 0], None),
            (b"O\0", 2, 2, [0, 0], [2, 2], None),
            (b"O\0", 3, 2, [0, 0], [1, 2], Some("v")),
            (b"O\0", 2, 3, [1, 2], [0, 0], Some("u")),
            (b"X\0", 3, 2, [0, 0], [0, 0], Some("job")),
            (b"\0\0", 3, 2, [0, 0], [0, 0], Some("job")),
        ] {
            let jobu = Rf_mkString(job.as_ptr().cast());
            let _j = protect(jobu);
            let x = make_input(&[n, p], &vec![(1., 0.); (n * p) as usize]);
            let _x = protect(x);
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, n.min(p) as R_xlen_t);
            let _s = protect(s);
            let u = make_input(&ud, &vec![(0., 0.); (ud[0] * ud[1]) as usize]);
            let _u = protect(u);
            let v = make_input(&vd, &vec![(0., 0.); (vd[0] * vd[1]) as usize]);
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
                La_svd_cmplx(jobu, x, s, u, v);
            })));
            if short == Some("job") {
                assert_eq!(message, "error code from Lapack routine 'zgesdd'");
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
#[cfg(feature = "rust-backend")]
fn complex_svd_admission_both_workspace_stages_deny_release_and_retry() {
    use crate::sexp::accessors::{CHAR, STRING_ELT};
    use crate::sexp::instance::with_current_instance;
    use crate::sexp::memory::with_arena_in;

    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        // Warm result names and lazy symbols without entering numerical SIMD,
        // which includes a NEON reduction not implemented by aarch64 Miri.
        let (ej, ex, es, eu, ev) = svd_args(0, 0, &[]);
        let _ej = protect(ej);
        let _ex = protect(ex);
        let _es = protect(es);
        let _eu = protect(eu);
        let _ev = protect(ev);
        let warm = La_svd_cmplx(ej, ex, es, eu, ev);
        let _warm = protect(warm);
        let (jobu, x, s, u, v) = svd_args(2, 2, &[(3., 0.), (0., 0.), (0., 0.), (0., 2.)]);
        let _j = protect(jobu);
        let _x = protect(x);
        let _s = protect(s);
        let _u = protect(u);
        let _v = protect(v);
        CHAR(STRING_ELT(jobu, 0));
        for matrix in [x, u, v] {
            crate::attrib_core::getAttrib(matrix, R_DimSymbol());
        }
        let (resident, held) = with_current_instance(|instance| {
            with_arena_in(instance, |arena| {
                (arena.total_bytes_allocated(), arena.workspace_bytes())
            })
        })
        .unwrap();
        // 4 complex copy elements + 14 real rwork + 112 integer iwork.
        let initial = 4 * size_of::<crate::modules::lapack::backend::LapRcomplex>()
            + 14 * size_of::<f64>()
            + 112 * size_of::<i32>();
        // The Rust backend queries 2*k*k + 2*k + max(m,n) complex elements.
        let queried = 14 * size_of::<crate::modules::lapack::backend::LapRcomplex>();
        for allowance in [initial - 1, initial + queried - 1] {
            *REAL(s) = -7.;
            *REAL(s).add(1) = -8.;
            with_current_instance(|instance| {
                with_arena_in(instance, |arena| {
                    arena.set_budget(ArenaBudget::new(resident + held + allowance, 0))
                })
            });
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_svd_cmplx(jobu, x, s, u, v);
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
        // Native coverage retains the full successful numerical retry. Miri
        // covers both preceding admission/unwind paths without faer's NEON.
        #[cfg(not(miri))]
        {
            let answer = La_svd_cmplx(jobu, x, s, u, v);
            let _answer = protect(answer);
            assert_eq!(
                std::slice::from_raw_parts(REAL(VECTOR_ELT(answer, 0)), 2),
                &[3., 2.]
            );
        }
        with_current_instance(|instance| {
            with_arena_in(instance, |arena| assert_eq!(arena.workspace_bytes(), held))
        });
    });
}

#[test]
#[cfg(all(feature = "rust-backend", not(miri)))]
fn complex_svd_overwrite_backend_rectangular_reconstruction_and_invalid_jobs() {
    use super::backend::{LapRcomplex as C, zgesdd_};
    for (m, n) in [(3i32, 2i32), (2, 3), (2, 2)] {
        let (rows, cols) = (m as usize, n as usize);
        let k = rows.min(cols);
        let mut a = vec![C::default(); rows * cols];
        a[0].r = 3.;
        a[1 + rows].i = 2.;
        let original = a.clone();
        let marker = C { r: 7., i: 9. };
        // Generous backing on BOTH sides makes the regression safe even if
        // overwrite dispatch regresses to writing thin outputs to both sides.
        let mut u = vec![marker; rows * rows];
        let mut vt = vec![marker; cols * cols];
        let mut s = vec![0.; k];
        let scratch = super::workspace_size::complex_svd_scratch(
            a.len(),
            false,
            k,
            rows.max(cols),
            size_of::<C>(),
        )
        .unwrap();
        let mut rwork = vec![0.; scratch.rwork_len];
        let mut iwork = vec![0; scratch.iwork_len];
        let mut query = C::default();
        let mut info = 99;
        unsafe {
            zgesdd_(
                &b'O',
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
                rwork.as_mut_ptr(),
                iwork.as_mut_ptr(),
                &mut info,
            );
        }
        assert_eq!(info, 0);
        let lwork = query.r as i32;
        let mut work = vec![C::default(); lwork as usize];
        unsafe {
            zgesdd_(
                &b'O',
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
                rwork.as_mut_ptr(),
                iwork.as_mut_ptr(),
                &mut info,
            );
        }
        assert_eq!(info, 0);
        assert!((s[0] - 3.).abs() < 1e-12 && (s[1] - 2.).abs() < 1e-12);
        let ignored = if rows >= cols { &u } else { &vt };
        assert!(ignored.iter().all(|z| z.r == marker.r && z.i == marker.i));
        // LAPACK O: A becomes thin U when tall/square, thin VH when wide.
        // Reconstructing the original avoids any basis/phase assumptions.
        for j in 0..cols {
            for i in 0..rows {
                let mut sum = C::default();
                for h in 0..k {
                    let left = if rows >= cols {
                        a[i + h * rows]
                    } else {
                        u[i + h * rows]
                    };
                    let right = if rows >= cols {
                        vt[h + j * cols]
                    } else {
                        a[h + j * rows]
                    };
                    sum.r += s[h] * (left.r * right.r - left.i * right.i);
                    sum.i += s[h] * (left.r * right.i + left.i * right.r);
                }
                let expected = original[i + j * rows];
                assert!((sum.r - expected.r).abs() < 1e-12 && (sum.i - expected.i).abs() < 1e-12);
            }
        }
        for job in [b'X', 0] {
            let before = a.clone();
            unsafe {
                zgesdd_(
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
                    rwork.as_mut_ptr(),
                    iwork.as_mut_ptr(),
                    &mut info,
                );
            }
            assert_eq!(info, -1);
            assert!(a.iter().zip(&before).all(|(a, b)| a.r == b.r && a.i == b.i));
        }
    }
}

#[test]
#[cfg(feature = "rust-backend")]
fn complex_svd_admission_overwrite_rectangular_workspace_stages() {
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
        let warm = La_svd_cmplx(ej, ex, es, eu, ev);
        let _warm = protect(warm);
        for (n, p) in [(3, 2), (2, 3)] {
            let jobu = crate::sexp::constructors::mk_string_str("O");
            let _j = protect(jobu);
            let mut values = vec![(0., 0.); (n * p) as usize];
            values[0] = (3., 0.);
            values[1 + n as usize] = (0., 2.);
            let x = make_input(&[n, p], &values);
            let _x = protect(x);
            let s = Rf_allocVector3(SEXPTYPE::REALSXP, 2);
            let _s = protect(s);
            let ud = if n >= p { [1, 1] } else { [n, n] };
            let vd = if n >= p { [p, p] } else { [1, 1] };
            let u = make_input(&ud, &vec![(7., 9.); (ud[0] * ud[1]) as usize]);
            let _u = protect(u);
            let v = make_input(&vd, &vec![(7., 9.); (vd[0] * vd[1]) as usize]);
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
            // Both rectangular shapes have k=2, max=3: initial1856, query240.
            for allowance in [1856 - 1, 1856 + 240 - 1] {
                *REAL(s) = -7.;
                *REAL(s).add(1) = -8.;
                with_current_instance(|instance| {
                    with_arena_in(instance, |arena| {
                        arena.set_budget(ArenaBudget::new(resident + held + allowance, 0))
                    })
                });
                let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                    La_svd_cmplx(jobu, x, s, u, v);
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
            #[cfg(not(miri))]
            {
                let answer = La_svd_cmplx(jobu, x, s, u, v);
                let _answer = protect(answer);
                let d = REAL(VECTOR_ELT(answer, 0));
                assert!((*d - 3.).abs() < 1e-12 && (*d.add(1) - 2.).abs() < 1e-12);
                let ignored = if n >= p { u } else { v };
                assert_eq!(((*COMPLEX(ignored)).r, (*COMPLEX(ignored)).i), (7., 9.));
                for (i, expected) in values.iter().enumerate() {
                    assert_eq!(((*COMPLEX(x).add(i)).r, (*COMPLEX(x).add(i)).i), *expected);
                }
            }
            with_current_instance(|instance| {
                with_arena_in(instance, |arena| assert_eq!(arena.workspace_bytes(), held))
            });
        }
    });
}
