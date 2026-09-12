//! Direct safety contracts for the complex solve embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_solve_cmplx;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{COMPLEX, INTEGER, SET_ATTRIB, SETTAG};
use crate::sexp::constructors::{Rf_ScalarReal, Rf_allocVector3, Rf_cons};
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

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid matrix should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

#[test]
fn complex_solve_accepts_zero_dimensional_square() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let a = make_input(&[0, 0], &[]);
        let b = make_input(&[0, 1], &[]);
        let tol = Rf_ScalarReal(-1.0);
        let _a = protect(a);
        let _b = protect(b);
        let _t = protect(tol);
        La_solve_cmplx(a, b, tol);
    });
}

#[test]
fn complex_solve_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let tol = Rf_ScalarReal(-1.0);
        let _t = protect(tol);
        let valid_b = make_input(&[2, 1], &[(1.0, 0.0), (2.0, 0.0)]);
        let _vb = protect(valid_b);
        for (dims, values) in [
            (vec![2], vec![(1.0, 0.0), (0.0, 0.0)]),
            (
                vec![2, 3],
                vec![
                    (1.0, 0.0),
                    (0.0, 0.0),
                    (0.0, 0.0),
                    (0.0, 0.0),
                    (1.0, 0.0),
                    (0.0, 0.0),
                ],
            ),
            (vec![2, 2], vec![(1.0, 0.0), (0.0, 0.0), (0.0, 0.0)]),
            (vec![-1, 2], vec![(1.0, 0.0), (0.0, 0.0)]),
            (vec![i32::MAX, i32::MAX], vec![(1.0, 0.0)]),
        ] {
            let a = make_input(&dims, &values);
            let _guard = protect(a);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_solve_cmplx(a, valid_b, tol);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }

        let a = make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]);
        let _a = protect(a);
        for (dims, values) in [
            (vec![2], vec![(1.0, 0.0), (2.0, 0.0)]),
            (vec![3, 1], vec![(1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]),
            (vec![2, 1], vec![(1.0, 0.0)]),
            (vec![-1, 1], vec![(1.0, 0.0)]),
        ] {
            let b = make_input(&dims, &values);
            let _guard = protect(b);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_solve_cmplx(a, b, tol);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }

        let a = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _a = protect(a);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve_cmplx(a, valid_b, tol);
        })));
        assert!(message.contains("complex matrix"), "{message}");

        let a = make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]);
        let _a2 = protect(a);
        let b = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _b = protect(b);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve_cmplx(a, b, tol);
        })));
        assert!(message.contains("complex matrix"), "{message}");
    });
}

#[test]
fn complex_solve_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (a, b, tol) = session.with_active(|| unsafe {
        let a = make_input(&[2, 2], &[(2.0, 0.0), (0.0, 0.0), (0.0, 0.0), (2.0, 0.0)]);
        let b = make_input(&[2, 1], &[(2.0, 2.0), (4.0, 0.0)]);
        let tol = Rf_ScalarReal(-1.0);
        (a, b, tol)
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _a = protect(a);
        let _b = protect(b);
        let _t = protect(tol);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_solve_cmplx(a, b, tol);
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
        let ans = La_solve_cmplx(a, b, tol);
        let _ans = protect(ans);
        assert!(((*COMPLEX(ans)).r - 1.0).abs() < 1e-12);
        assert!(((*COMPLEX(ans)).i - 1.0).abs() < 1e-12);
        assert!(((*COMPLEX(ans).add(1)).r - 2.0).abs() < 1e-12);
        assert!(((*COMPLEX(ans).add(1)).i - 0.0).abs() < 1e-12);
    });
}
