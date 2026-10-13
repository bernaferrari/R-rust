use super::*;
use crate::sexp::accessors::{INTEGER, LENGTH, LOGICAL, REAL, TYPEOF};
use crate::sexp::context::RSignal;
use crate::sexp::session::{CancellationToken, RSession};

fn expect_cancelled<F>(f: F)
where
    F: FnOnce(),
{
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let Err(payload) = result else {
        panic!("expected cancellation");
    };
    let Some(RSignal::Error { message }) = payload.downcast_ref::<RSignal>() else {
        panic!("expected RSignal::Error payload");
    };
    assert_eq!(message, "operation cancelled");
}

#[test]
fn test_scalar_addition() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(1);
        let b = Rf_ScalarInteger(2);
        let result = real_binary("+", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(result), 3);
    }
}

#[test]
fn test_real_addition() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarReal(1.5);
        let b = Rf_ScalarReal(2.5);
        let result = real_binary("+", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        let v = *REAL(result);
        assert!((v - 4.0).abs() < 1e-10);
    }
}

#[test]
fn test_scalar_multiplication() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(3);
        let b = Rf_ScalarInteger(4);
        let result = real_binary("*", a, b);
        assert_eq!(*INTEGER(result), 12);
    }
}

#[test]
fn test_division_produces_real() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(10);
        let b = Rf_ScalarInteger(3);
        let result = real_binary("/", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        let v = *REAL(result);
        assert!((v - 3.3333333333333335).abs() < 1e-10);
    }
}

#[test]
fn test_division_by_zero_matches_r_real_semantics() {
    let _session = RSession::new();
    unsafe {
        let zero = Rf_ScalarInteger(0);

        let pos = real_binary("/", Rf_ScalarInteger(1), zero);
        assert_eq!(TYPEOF(pos), SEXPTYPE::REALSXP);
        assert!((*REAL(pos)).is_infinite());
        assert!((*REAL(pos)).is_sign_positive());

        let neg = real_binary("/", Rf_ScalarInteger(-1), zero);
        assert_eq!(TYPEOF(neg), SEXPTYPE::REALSXP);
        assert!((*REAL(neg)).is_infinite());
        assert!((*REAL(neg)).is_sign_negative());

        let nan = real_binary("/", Rf_ScalarInteger(0), zero);
        assert_eq!(TYPEOF(nan), SEXPTYPE::REALSXP);
        assert!((*REAL(nan)).is_nan());
        assert_ne!((*REAL(nan)).to_bits(), NA_REAL.to_bits());
    }
}

#[test]
fn test_scalar_power() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(2);
        let b = Rf_ScalarInteger(10);
        let result = real_binary("^", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        assert_eq!(*REAL(result), 1024.0);
    }
}

#[test]
fn test_comparison_lt() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(1);
        let b = Rf_ScalarInteger(2);
        let result = binary_compare("<", a, b);
        assert_eq!(*LOGICAL(result), TRUE);
    }
}

#[test]
fn test_comparison_eq() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(5);
        let b = Rf_ScalarInteger(5);
        let result = binary_compare("==", a, b);
        assert_eq!(*LOGICAL(result), TRUE);
    }
}

#[test]
fn test_comparison_ne() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarReal(1.0);
        let b = Rf_ScalarReal(2.0);
        let result = binary_compare("!=", a, b);
        assert_eq!(*LOGICAL(result), TRUE);
    }
}

#[test]
fn test_scalar_modulo() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(10);
        let b = Rf_ScalarInteger(3);
        let result = real_binary("%%", a, b);
        assert_eq!(*INTEGER(result), 1);
    }
}

#[test]
fn test_scalar_integer_division() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(10);
        let b = Rf_ScalarInteger(3);
        let result = real_binary("%/%", a, b);
        assert_eq!(*INTEGER(result), 3);
    }
}

#[test]
fn test_modulo_and_floor_division_by_zero_match_r_shape() {
    let _session = RSession::new();
    unsafe {
        let int_zero = Rf_ScalarInteger(0);

        let int_mod = real_binary("%%", Rf_ScalarInteger(1), int_zero);
        assert_eq!(TYPEOF(int_mod), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(int_mod), NA_INTEGER);

        let int_floor_div = real_binary("%/%", Rf_ScalarInteger(1), int_zero);
        assert_eq!(TYPEOF(int_floor_div), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(int_floor_div), NA_INTEGER);

        let real_zero = Rf_ScalarReal(0.0);

        let real_mod = real_binary("%%", Rf_ScalarReal(1.0), real_zero);
        assert_eq!(TYPEOF(real_mod), SEXPTYPE::REALSXP);
        assert!((*REAL(real_mod)).is_nan());
        assert_ne!((*REAL(real_mod)).to_bits(), NA_REAL.to_bits());

        let real_floor_div = real_binary("%/%", Rf_ScalarReal(-1.0), real_zero);
        assert_eq!(TYPEOF(real_floor_div), SEXPTYPE::REALSXP);
        assert!((*REAL(real_floor_div)).is_infinite());
        assert!((*REAL(real_floor_div)).is_sign_negative());
    }
}

// --- Vector tests ---

#[test]
fn test_vector_addition_with_recycling() {
    let _session = RSession::new();
    unsafe {
        // c(1,2,3) + c(10,20) should recycle → c(11, 22, 13)
        let a = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        *INTEGER(a).add(0) = 1;
        *INTEGER(a).add(1) = 2;
        *INTEGER(a).add(2) = 3;

        let b = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(b).add(0) = 10;
        *INTEGER(b).add(1) = 20;

        let result = real_binary("+", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(LENGTH(result), 3);
        assert_eq!(*INTEGER(result).add(0), 11); // 1+10
        assert_eq!(*INTEGER(result).add(1), 22); // 2+20
        assert_eq!(*INTEGER(result).add(2), 13); // 3+10 (recycled)
    }
}

#[test]
fn test_vector_comparison() {
    let _session = RSession::new();
    unsafe {
        // c(1,2,3) > c(2,1,2) → c(FALSE, TRUE, TRUE)
        let a = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        *INTEGER(a).add(0) = 1;
        *INTEGER(a).add(1) = 2;
        *INTEGER(a).add(2) = 3;

        let b = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        *INTEGER(b).add(0) = 2;
        *INTEGER(b).add(1) = 1;
        *INTEGER(b).add(2) = 2;

        let result = binary_compare(">", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
        assert_eq!(LENGTH(result), 3);
        assert_eq!(*LOGICAL(result).add(0), FALSE); // 1 > 2
        assert_eq!(*LOGICAL(result).add(1), TRUE); // 2 > 1
        assert_eq!(*LOGICAL(result).add(2), TRUE); // 3 > 2
    }
}

#[test]
fn data_frame_comparison_is_columnwise_and_preserves_labels() {
    let mut session = RSession::new();
    let (result, output, visible) = session.eval_code_with_output_capture(
        "x <- matrix(1:4, 2, 2, dimnames=list(c('abc','ab'), c('cde','cd'))); \
             y <- as.data.frame(x); \
             row <- y['ab',] == c(2L, 4L); \
             reverse <- c(2L, 4L) == y['ab',]; \
             listwise <- data.frame(a=1:2, b=3:4) > list(1L, 3L); \
             splitwise <- data.frame(a=1:2, b=3:4) == 1:4; \
             identical(row, matrix(c(TRUE, TRUE), 1L, 2L, \
                       dimnames=list('ab', c('cde','cd')))) && \
             identical(reverse, row) && \
             identical(listwise, matrix(c(FALSE, TRUE, FALSE, TRUE), 2L, 2L, \
                       dimnames=list(NULL, c('a','b')))) && \
             identical(splitwise, matrix(rep(TRUE, 4L), 2L, 2L, \
                       dimnames=list(NULL, c('a','b'))))",
    );
    let result = result.expect("data-frame comparisons should evaluate");
    assert_eq!(result.logical_elt(0), Some(TRUE));
    assert!(output.stdout.is_empty());
    assert!(visible);
}

#[test]
fn test_vector_arithmetic_polls_cancellation() {
    let mut session = RSession::new();
    session.set_cancellation_token(Some(CancellationToken::cancelled()));

    unsafe {
        let a = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
        let b = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
        for i in 0..2048 {
            *INTEGER(a).add(i) = i as i32;
            *INTEGER(b).add(i) = 1;
        }

        expect_cancelled(|| {
            let _ = real_binary("+", a, b);
        });
    }
}

#[test]
fn test_vector_compare_polls_cancellation() {
    let mut session = RSession::new();
    session.set_cancellation_token(Some(CancellationToken::cancelled()));

    unsafe {
        let a = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
        let b = Rf_allocVector3(SEXPTYPE::INTSXP, 2048);
        for i in 0..2048 {
            *INTEGER(a).add(i) = i as i32;
            *INTEGER(b).add(i) = i as i32;
        }

        expect_cancelled(|| {
            let _ = binary_compare("==", a, b);
        });
    }
}

#[test]
fn test_unary_math_polls_cancellation() {
    let mut session = RSession::new();
    session.set_cancellation_token(Some(CancellationToken::cancelled()));

    unsafe {
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, 2048);
        for i in 0..2048 {
            *REAL(x).add(i) = i as f64;
        }

        expect_cancelled(|| {
            let _ = math1_vec(R_NilValue(), x, f64::sqrt);
        });
    }
}

#[test]
fn test_vector_na_propagation() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarInteger(NA_INTEGER);
        let b = Rf_ScalarInteger(5);
        let result = real_binary("+", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(result), NA_INTEGER);
    }
}

#[test]
fn test_scalar_plus_vector() {
    let _session = RSession::new();
    unsafe {
        // 1 + c(10, 20, 30) → c(11, 21, 31)
        let a = Rf_ScalarInteger(1);
        let b = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        *INTEGER(b).add(0) = 10;
        *INTEGER(b).add(1) = 20;
        *INTEGER(b).add(2) = 30;

        let result = real_binary("+", a, b);
        assert_eq!(LENGTH(result), 3);
        assert_eq!(*INTEGER(result).add(0), 11);
        assert_eq!(*INTEGER(result).add(1), 21);
        assert_eq!(*INTEGER(result).add(2), 31);
    }
}

#[test]
fn test_unary_minus_vector() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        *INTEGER(a).add(0) = 1;
        *INTEGER(a).add(1) = -5;
        *INTEGER(a).add(2) = 0;

        let result = unary_minus(a);
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(*INTEGER(result).add(0), -1);
        assert_eq!(*INTEGER(result).add(1), 5);
        assert_eq!(*INTEGER(result).add(2), 0);
    }
}

#[test]
fn test_real_plus_int_produces_real() {
    let _session = RSession::new();
    unsafe {
        let a = Rf_ScalarReal(1.5);
        let b = Rf_ScalarInteger(2);
        let result = real_binary("+", a, b);
        assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
        assert!((*REAL(result) - 3.5).abs() < 1e-10);
    }
}
