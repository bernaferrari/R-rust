//! Direct safety contracts for the real QR embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_qr;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{INTEGER, REAL, SET_ATTRIB, SETTAG};
use crate::sexp::constructors::{Rf_allocVector3, Rf_cons};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::protect::protect;
use crate::sexp::session::RSession;

unsafe fn make_input(dimensions: &[i32], data_len: usize) -> SEXP {
    unsafe {
        let data = Rf_allocVector3(SEXPTYPE::REALSXP, data_len as R_xlen_t);
        let _data_guard = protect(data);
        let dims = Rf_allocVector3(SEXPTYPE::INTSXP, dimensions.len() as R_xlen_t);
        let _dims_guard = protect(dims);
        for (i, value) in dimensions.iter().enumerate() {
            *INTEGER(dims).add(i) = *value;
        }
        let attrs = Rf_cons(dims, R_NilValue());
        SETTAG(attrs, R_DimSymbol());
        SET_ATTRIB(data, attrs);
        for i in 0..data_len {
            *REAL(data).add(i) = i as f64 + 1.0;
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
fn real_qr_accepts_zero_dimensional_shapes() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        for dims in [[0, 3], [3, 0]] {
            let input = make_input(&dims, 0);
            let _guard = protect(input);
            La_qr(input);
        }
    });
}

#[test]
fn real_qr_rejects_short_dimensions_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        for (dims, len) in [(vec![2], 2), (vec![2, 2], 3), (vec![-1, 2], 2)] {
            let input = make_input(&dims, len);
            let _guard = protect(input);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_qr(input);
            })));
            assert!(message.contains("matrix"), "{message}");
        }
        let input = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_qr(input);
        })));
        assert!(message.contains("numeric matrix"), "{message}");
    });
}

#[test]
fn real_qr_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let input = session.with_active(|| unsafe { make_input(&[2, 1], 2) });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_qr(input);
        })));
        assert!(
            message.contains("native QR workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        La_qr(input);
    });
}
