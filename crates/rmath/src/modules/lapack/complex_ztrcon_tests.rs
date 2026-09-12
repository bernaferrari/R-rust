//! Direct safety contracts for the complex triangular condition-number boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::{La_ztrcon, La_ztrcon3};
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{COMPLEX, INTEGER, REAL, SET_ATTRIB, SETTAG};
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

unsafe fn one_norm() -> SEXP {
    unsafe { Rf_mkString(b"O\0".as_ptr() as *const std::os::raw::c_char) }
}

unsafe fn upper() -> SEXP {
    unsafe { Rf_mkString(b"U\0".as_ptr() as *const std::os::raw::c_char) }
}

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid matrix should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

#[test]
fn complex_ztrcon_accepts_zero_dimensional_square() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let input = make_input(&[0, 0], &[]);
        let _guard = protect(input);
        let norm = one_norm();
        let _n = protect(norm);
        let ans = La_ztrcon(input, norm);
        let _ans = protect(ans);
        assert!((*REAL(ans)).is_finite() || (*REAL(ans)).is_infinite());
    });
}

#[test]
fn complex_ztrcon_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let norm = one_norm();
        let _n = protect(norm);
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
            let input = make_input(&dims, &values);
            let _guard = protect(input);
            let message = error_message(catch_unwind(AssertUnwindSafe(|| {
                La_ztrcon(input, norm);
            })));
            assert!(message.contains("matrix"), "{message}");
        }
        let input = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_ztrcon(input, norm);
        })));
        assert!(message.contains("complex matrix"), "{message}");
    });
}

#[test]
fn complex_ztrcon_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (input, norm) = session.with_active(|| unsafe {
        (
            make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]),
            one_norm(),
        )
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _n = protect(norm);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_ztrcon(input, norm);
        })));
        assert!(
            message.contains(
                "native complex triangular condition-number workspace exceeds resource limit"
            ),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _n = protect(norm);
        let ans = La_ztrcon(input, norm);
        let _ans = protect(ans);
        // The rust backend approximates rcond as min|diag|/(max|diag|*n).
        let rcond = *REAL(ans);
        assert!(rcond.is_finite() && rcond > 0.0 && rcond <= 1.0, "{rcond}");
    });
}

#[test]
fn complex_ztrcon3_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (input, norm, uplo) = session.with_active(|| unsafe {
        (
            make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]),
            one_norm(),
            upper(),
        )
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _n = protect(norm);
        let _u = protect(uplo);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_ztrcon3(input, norm, uplo);
        })));
        assert!(
            message.contains(
                "native complex triangular condition-number workspace exceeds resource limit"
            ),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _n = protect(norm);
        let _u = protect(uplo);
        let ans = La_ztrcon3(input, norm, uplo);
        let _ans = protect(ans);
        let rcond = *REAL(ans);
        assert!(rcond.is_finite() && rcond > 0.0 && rcond <= 1.0, "{rcond}");
    });
}
