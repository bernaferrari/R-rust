//! Direct safety contracts for the real solve embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_solve;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{INTEGER, REAL, SET_ATTRIB, SETTAG};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3, Rf_cons};
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

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid matrix should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

#[test]
fn real_solve_accepts_zero_dimensional_square() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let a = make_input(&[0, 0], &[]);
        let b = make_input(&[0, 1], &[]);
        let tol = Rf_ScalarReal(-1.0);
        let _a = protect(a);
        let _b = protect(b);
        let _t = protect(tol);
        La_solve(a, b, tol);
    });
}

#[test]
fn real_solve_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let tol = Rf_ScalarReal(-1.0);
        let _t = protect(tol);
        let valid_b = make_input(&[2, 1], &[1.0, 2.0]);
        let _vb = protect(valid_b);
        for (dims, values) in [
            (vec![2], vec![1.0, 0.0]),
            (vec![2, 3], vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            (vec![2, 2], vec![1.0, 0.0, 0.0]),
            (vec![-1, 2], vec![1.0, 0.0]),
            (vec![i32::MAX, i32::MAX], vec![1.0]),
        ] {
            let a = make_input(&dims, &values);
            let _guard = protect(a);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_solve(a, valid_b, tol);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }

        let a = make_input(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        let _a = protect(a);
        for (dims, values) in [
            (vec![2], vec![1.0, 2.0]),
            (vec![3, 1], vec![1.0, 2.0, 3.0]),
            (vec![2, 1], vec![1.0]),
            (vec![-1, 1], vec![1.0]),
        ] {
            let b = make_input(&dims, &values);
            let _guard = protect(b);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_solve(a, b, tol);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }

        let a = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _a = protect(a);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve(a, valid_b, tol);
        })));
        assert!(message.contains("numeric matrix"), "{message}");

        let a = make_input(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        let _a2 = protect(a);
        let b = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _b = protect(b);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve(a, b, tol);
        })));
        assert!(message.contains("numeric matrix"), "{message}");
    });
}

#[test]
fn real_solve_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (a, b, tol) = session.with_active(|| unsafe {
        let a = make_input(&[2, 2], &[2.0, 0.0, 0.0, 2.0]);
        let b = make_input(&[2, 1], &[2.0, 4.0]);
        let tol = Rf_ScalarReal(-1.0);
        (a, b, tol)
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _a = protect(a);
        let _b = protect(b);
        let _t = protect(tol);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve(a, b, tol);
        })));
        assert!(
            message.contains("native solve workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _a = protect(a);
        let _b = protect(b);
        let _t = protect(tol);
        let ans = La_solve(a, b, tol);
        let _ans = protect(ans);
        assert!((*REAL(ans) - 1.0).abs() < 1e-12);
        assert!((*REAL(ans).add(1) - 2.0).abs() < 1e-12);
    });
}

#[test]
fn real_solve_tolerance_workspace_denial_and_numerical_unwind_release() {
    use crate::sexp::instance::with_current_instance;
    use crate::sexp::memory::with_arena_in;
    use crate::sexp::memory_ext::{vmaxget, vmaxset};

    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let a = make_input(&[2, 2], &[2.0, 0.0, 0.0, 2.0]);
        let _a = protect(a);
        let singular = make_input(&[2, 2], &[1.0, 1.0, 1.0, 1.0]);
        let _singular = protect(singular);
        let b = make_input(&[2, 1], &[2.0, 4.0]);
        let _b = protect(b);
        let tol = Rf_ScalarReal(1e-12);
        let _tol = protect(tol);
        let strict_tol = Rf_ScalarReal(2.0);
        let _strict_tol = protect(strict_tol);
        // The lightweight fixture initializes symbols and formatted-error
        // caches lazily. Warm these paths before measuring caller workspace.
        let initial_watermark = vmaxget();
        La_solve(a, b, tol);
        vmaxset(initial_watermark);
        for (matrix, tolerance) in [(singular, tol), (a, strict_tol)] {
            let _ = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_solve(matrix, b, tolerance);
            })));
            vmaxset(initial_watermark);
        }
        let watermark = vmaxget();
        let (resident, held) = with_current_instance(|instance| {
            with_arena_in(instance, |arena| {
                (arena.total_bytes_allocated(), arena.workspace_bytes())
            })
        })
        .unwrap();

        // Two dense copies need 48 bytes; positive tolerance additionally
        // retains 8 doubles and 2 integers. One byte below that full caller
        // peak must fail at admission, before pivots or numerical work begin.
        let caller_bytes = 6 * size_of::<f64>() + 8 * size_of::<f64>() + 2 * size_of::<i32>();
        with_current_instance(|instance| {
            with_arena_in(instance, |arena| {
                arena.set_budget(ArenaBudget::new(resident + held + caller_bytes - 1, 0));
            })
        });
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve(a, b, tol);
        })));
        assert!(
            message.contains("native solve workspace exceeds resource limit"),
            "{message}"
        );
        assert_eq!(vmaxget(), watermark);
        with_current_instance(|instance| {
            with_arena_in(instance, |arena| {
                assert_eq!(arena.workspace_bytes(), held);
                arena.set_budget(ArenaBudget::unlimited());
            })
        });

        for (matrix, tolerance, expected) in [
            (singular, tol, "singular matrix in 'solve'"),
            (a, strict_tol, "system is computationally singular"),
        ] {
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_solve(matrix, b, tolerance);
            })));
            assert!(message.contains(expected), "{message}");
            // R_alloc pivots live to the caller's watermark; the Vec scratch
            // reservation must already be gone when the numerical error unwinds.
            with_current_instance(|instance| {
                with_arena_in(instance, |arena| {
                    assert_eq!(arena.workspace_bytes(), held + 2 * size_of::<i32>());
                })
            });
            vmaxset(watermark);
        }

        let answer = La_solve(a, b, tol);
        let _answer = protect(answer);
        assert_eq!(std::slice::from_raw_parts(REAL(answer), 2), &[1.0, 2.0]);
        vmaxset(watermark);
        with_current_instance(|instance| {
            with_arena_in(instance, |arena| assert_eq!(arena.workspace_bytes(), held))
        });
    });
}
