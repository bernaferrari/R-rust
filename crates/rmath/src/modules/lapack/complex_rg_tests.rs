//! Direct safety contracts for the complex general eigen embedding boundary.
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::lapack_impl::La_rg_cmplx;
use crate::attrib_core::R_DimSymbol;
use crate::sexp::accessors::{COMPLEX, INTEGER, SET_ATTRIB, SETTAG};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3, Rf_cons};
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
fn complex_rg_accepts_zero_dimensional_square() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let input = make_input(&[0, 0], &[]);
        let _guard = protect(input);
        let only = Rf_ScalarLogical(1);
        let _o = protect(only);
        La_rg_cmplx(input, only);
    });
}

#[test]
fn complex_rg_rejects_malformed_dims_and_payload() {
    let session = RSession::new();
    session.with_active(|| unsafe {
        let only = Rf_ScalarLogical(1);
        let _o = protect(only);
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
                La_rg_cmplx(input, only);
            })));
            assert!(message.contains("matrix"), "{message}");
        }
        let input = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        let _guard = protect(input);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_rg_cmplx(input, only);
        })));
        assert!(message.contains("complex matrix"), "{message}");
    });
}

#[test]
fn complex_rg_reserves_caller_scratch_before_allocation_and_recovers() {
    let mut session = RSession::new();
    let (input, only) = session.with_active(|| unsafe {
        (
            make_input(&[2, 2], &[(1.0, 0.0), (0.0, 0.0), (0.0, 0.0), (1.0, 0.0)]),
            Rf_ScalarLogical(1),
        )
    });
    session.set_arena_budget(ArenaBudget::new(1, 0));
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _o = protect(only);
        let message = error_message(catch_unwind(AssertUnwindSafe(|| {
            La_rg_cmplx(input, only);
        })));
        assert!(
            message.contains("native complex eigen workspace exceeds resource limit"),
            "{message}"
        );
    });
    session.set_arena_budget(ArenaBudget::unlimited());
    session.with_active(|| unsafe {
        let _guard = protect(input);
        let _o = protect(only);
        let ans = La_rg_cmplx(input, only);
        let _ans = protect(ans);
        assert!(!ans.is_null());
    });
}
