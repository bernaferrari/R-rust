//! Direct safety contracts for the complex QR Q*y embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::qr_qy_cmplx;
use crate::attrib_core::{R_DimSymbol, R_NamesSymbol};
use crate::sexp::accessors::{COMPLEX, INTEGER, SET_ATTRIB, SET_STRING_ELT, SET_VECTOR_ELT, SETTAG};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_cons, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::protect::protect;
use crate::sexp::session::RSession;

unsafe fn make_matrix(dimensions: &[i32], values: &[(f64, f64)]) -> SEXP {
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

unsafe fn make_complex_vector(values: &[(f64, f64)]) -> SEXP {
    unsafe {
        let data = Rf_allocVector3(SEXPTYPE::CPLXSXP, values.len() as R_xlen_t);
        let _data_guard = protect(data);
        for (i, (re, im)) in values.iter().enumerate() {
            (*COMPLEX(data).add(i)).r = *re;
            (*COMPLEX(data).add(i)).i = *im;
        }
        data
    }
}

unsafe fn make_qr(qr: SEXP, qraux: SEXP) -> SEXP {
    unsafe {
        let obj = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _obj = protect(obj);
        SET_VECTOR_ELT(obj, 0, qr);
        SET_VECTOR_ELT(obj, 1, R_NilValue());
        SET_VECTOR_ELT(obj, 2, qraux);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        let _names = protect(names);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"qr\0".as_ptr() as *const std::os::raw::c_char));
        SET_STRING_ELT(names, 1, Rf_mkChar(b"rank\0".as_ptr() as *const std::os::raw::c_char));
        SET_STRING_ELT(names, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const std::os::raw::c_char));
        SET_ATTRIB(obj, {
            let attrs = Rf_cons(names, R_NilValue());
            SETTAG(attrs, R_NamesSymbol());
            attrs
        });
        obj
    }
}

fn error_message(result: Result<(), Box<dyn std::any::Any + Send>>) -> String {
    result
        .expect_err("invalid QR Qy call should fail")
        .downcast::<crate::sexp::context::RError>()
        .expect("must fail with an R error")
        .message
}

#[test]
fn complex_qr_qy_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let qr = make_qr(
            make_matrix(
                &[2, 2],
                &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)],
            ),
            make_complex_vector(&[(0.0, 0.0), (0.0, 0.0)]),
        );
        let _qr = protect(qr);
        let trans = Rf_ScalarLogical(0);
        let _trans = protect(trans);

        let y = make_matrix(&[2], &[(1.0, 0.0), (2.0, 0.0)]);
        let _y = protect(y);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            qr_qy_cmplx(qr, y, trans);
        })));
        assert!(message.contains("matrix"), "{message}");

        let y = make_matrix(&[3, 1], &[(1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]);
        let _y2 = protect(y);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            qr_qy_cmplx(qr, y, trans);
        })));
        assert!(
            message.contains("matrix") || message.contains("dimension"),
            "{message}"
        );

        let y = make_matrix(&[2, 1], &[(1.0, 0.0), (2.0, 0.0)]);
        let _y3 = protect(y);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            qr_qy_cmplx(y, y, trans);
        })));
        assert!(message.contains("QR"), "{message}");
    });
}

#[test]
fn complex_qr_qy_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (qr, y, trans) = session.with_active(|| unsafe {
        (
            make_qr(
                make_matrix(
                    &[2, 2],
                    &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)],
                ),
                make_complex_vector(&[(0.0, 0.0), (0.0, 0.0)]),
            ),
            make_matrix(&[2, 1], &[(3.0, 0.0), (4.0, 0.0)]),
            Rf_ScalarLogical(0),
        )
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _qr = protect(qr);
        let _y = protect(y);
        let _trans = protect(trans);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            qr_qy_cmplx(qr, y, trans);
        })));
        assert!(
            message.contains("native QR Qy workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _qr = protect(qr);
        let _y = protect(y);
        let _trans = protect(trans);
        let ans = qr_qy_cmplx(qr, y, trans);
        let _ans = protect(ans);
        assert!(((*COMPLEX(ans)).r - 3.0).abs() < 1e-10);
        assert!(((*COMPLEX(ans)).i).abs() < 1e-10);
        assert!(((*COMPLEX(ans).add(1)).r - 4.0).abs() < 1e-10);
        assert!(((*COMPLEX(ans).add(1)).i).abs() < 1e-10);
    });
}

