//! Direct safety contracts for the real Cholesky-inverse embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_chol2inv;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{INTEGER, REAL, SET_ATTRIB, SETTAG};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3, Rf_cons};
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
fn real_chol2inv_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let size = Rf_ScalarInteger(2);
        let _size = protect(size);
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
                La_chol2inv(input, size);
            })));
            assert!(
                message.contains("matrix") || message.contains("dimension"),
                "{message}"
            );
        }
        let input = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_chol2inv(input, size);
        })));
        assert!(message.contains("numeric matrix"), "{message}");
        let input = make_input(&[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        let _ok = protect(input);
        let bad_size = Rf_ScalarInteger(3);
        let _bs = protect(bad_size);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_chol2inv(input, bad_size);
        })));
        assert!(
            message.contains("matrix") || message.contains("dimension"),
            "{message}"
        );
    });
}

#[test]
fn real_chol2inv_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (input, size) = session.with_active(|| unsafe {
        let scale = 2.0_f64.sqrt();
        let input = make_input(&[2, 2], &[scale, 0.0, 0.0, scale]);
        let size = Rf_ScalarInteger(2);
        (input, size)
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _size = protect(size);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_chol2inv(input, size);
        })));
        assert!(
            message.contains("native Cholesky inverse workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _size = protect(size);
        let ans = La_chol2inv(input, size);
        let _ans = protect(ans);
        assert!((*REAL(ans) - 0.5).abs() < 1e-12);
        assert!((*REAL(ans).add(1)).abs() < 1e-12);
        assert!((*REAL(ans).add(2)).abs() < 1e-12);
        assert!((*REAL(ans).add(3) - 0.5).abs() < 1e-12);
    });
}
