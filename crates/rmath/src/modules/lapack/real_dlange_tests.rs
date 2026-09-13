//! Direct safety contracts for the real matrix-norm embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_dlange;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{INTEGER, REAL, SET_ATTRIB, SETTAG};
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

unsafe fn one_norm() -> SEXP {
    unsafe { Rf_mkString(b"O\0".as_ptr() as *const std::os::raw::c_char) }
}

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid matrix should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

#[test]
fn real_dlange_accepts_zero_dimensional_shapes() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let typ = one_norm();
        let _t = protect(typ);
        for dims in [[0, 3], [3, 0], [0, 0]] {
            let input = make_input(&dims, &[]);
            let _guard = protect(input);
            let ans = La_dlange(input, typ);
            let _ans = protect(ans);
            assert!((*REAL(ans)).is_finite());
        }
    });
}

#[test]
fn real_dlange_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let typ = one_norm();
        let _t = protect(typ);
        for (dims, values) in [
            (vec![2], vec![1.0, 0.0]),
            (vec![2, 2], vec![1.0, 0.0, 0.0]),
            (vec![-1, 2], vec![1.0, 0.0]),
            (vec![i32::MAX, i32::MAX], vec![1.0]),
        ] {
            let input = make_input(&dims, &values);
            let _guard = protect(input);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_dlange(input, typ);
            })));
            assert!(message.contains("matrix"), "{message}");
        }
        let input = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_dlange(input, typ);
        })));
        assert!(message.contains("numeric matrix"), "{message}");
    });
}

#[test]
fn real_dlange_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (input, typ) = session.with_active(|| unsafe {
        (make_input(&[2, 2], &[1.0, 0.0, 0.0, 1.0]), one_norm())
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _t = protect(typ);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_dlange(input, typ);
        })));
        assert!(
            message.contains("native matrix-norm workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _t = protect(typ);
        let ans = La_dlange(input, typ);
        let _ans = protect(ans);
        assert!((*REAL(ans) - 1.0).abs() < 1e-12);
    });
}
