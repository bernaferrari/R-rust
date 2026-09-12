//! GNU R 4.6.1 public QR API for complex matrices.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R version 4.6.1 (2026-06-24)`).
//! Real/Linpack QR is already pinned; `qr(matrix(1i+1, 2, 2))` still errors.

use r_embed::RSession;

#[test]
fn complex_qr_matches_gnu_shape_rank_and_pivot() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "q <- qr(matrix(1+1i, 2, 2));              c(identical(class(q), 'qr'), identical(q$rank, 2L),                identical(q$pivot, 1:2), identical(typeof(q$qr), 'complex'),                identical(dim(q$qr), c(2L, 2L)), identical(length(q$qraux), 2L))",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE TRUE TRUE");
}

#[test]
fn complex_qr_values_match_gnu() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "q <- qr(matrix(1+1i, 2, 2));              c(max(abs(q$qr - matrix(c(-2+0i, 0.4+0.2i, -2+0i, 0+0i), 2, 2))) < 1e-8,                max(abs(q$qraux - c(1.5+0.5i, 1.89442719099992+0.447213595499958i))) < 1e-8)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE");
}

#[test]
fn complex_qr_q_r_and_coef_match_gnu() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "q <- qr(matrix(1+1i, 2, 2));              cf <- qr.coef(q, c(1+0i, 1+0i));              c(identical(dim(qr.Q(q)), c(2L, 2L)), identical(dim(qr.R(q)), c(2L, 2L)),                max(abs(cf - c(-0.75+0.125i, 1.25-0.625i))) < 1e-10)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE");
}
