//! Direct safety contracts for the real Cholesky embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_chol;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{INTEGER, REAL, SET_ATTRIB, SETTAG};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons};
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

unsafe fn no_pivot_tol() -> (SEXP, SEXP) {
    unsafe { (Rf_ScalarLogical(0), Rf_ScalarReal(-1.0)) }
}

#[test]
fn real_chol_accepts_zero_dimensional_square() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let input = make_input(&[0, 0], &[]);
        let _guard = protect(input);
        let (pivot, tol) = no_pivot_tol();
        let _p = protect(pivot);
        let _t = protect(tol);
        La_chol(input, pivot, tol);
    });
}

#[test]
fn real_chol_rejects_malformed_dims_and_short_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let (pivot, tol) = no_pivot_tol();
        let _p = protect(pivot);
        let _t = protect(tol);
        for (dims, values) in [
            (vec![2], vec![1.0, 0.0]),
            (vec![2, 3], vec![2.0, 0.0, 0.0, 0.0, 2.0, 0.0]),
            (vec![2, 2], vec![2.0, 0.0, 0.0]),
            (vec![-1, 2], vec![1.0, 0.0]),
            (vec![i32::MAX, i32::MAX], vec![1.0]),
        ] {
            let input = make_input(&dims, &values);
            let _guard = protect(input);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_chol(input, pivot, tol);
            })));
            assert!(message.contains("matrix"), "{message}");
        }
        let input = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_chol(input, pivot, tol);
        })));
        assert!(message.contains("numeric matrix"), "{message}");
    });
}

#[test]
fn real_chol_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (input, pivot, tol) = session.with_active(|| unsafe {
        let input = make_input(&[2, 2], &[2.0, 0.0, 0.0, 2.0]);
        let (pivot, tol) = no_pivot_tol();
        (input, pivot, tol)
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _p = protect(pivot);
        let _t = protect(tol);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_chol(input, pivot, tol);
        })));
        assert!(
            message.contains("native Cholesky workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _p = protect(pivot);
        let _t = protect(tol);
        let ans = La_chol(input, pivot, tol);
        let _ans = protect(ans);
        assert!((*REAL(ans) - 2.0_f64.sqrt()).abs() < 1e-12);
    });
}
