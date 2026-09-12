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
