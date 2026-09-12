//! GNU R public QR API for complex matrices.
//!
//! Values are pinned against both R builds this repo trusts: the pinned
//! trunk oracle (r90451, reference LAPACK) and Homebrew 4.6.1 (Accelerate
//! LAPACK).  The two builds only disagree on numerically degenerate
//! factorizations (duplicate columns): reference LAPACK reports
//! `qraux[2] == 0` and a ztrtrs error where Accelerate keeps noise-driven
//! values, so those unstable surfaces are deliberately not pinned.
//! rport implements reference LAPACK (zlarfg/zlaqp2) semantics.

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
            "q <- qr(matrix(1+1i, 2, 2));              stable <- max(abs(q$qr - matrix(c(-2+0i, 0.4+0.2i, -2+0i, 0+0i), 2, 2))) < 1e-8 &&                max(abs(q$qraux[1] - (1.5+0.5i))) < 1e-8;              a <- matrix(c(1+1i, 2+1i, 3+2i, 4+2i), 2, 2);              q2 <- qr(a);              full <- max(abs(q2$qr - matrix(c(-5.7445626+0i, 0.4843982+0.1179251i,                                         -2.6111648-0.1740777i, -0.3892495+0i), 2, 2))) < 1e-6 &&                max(abs(q2$qraux - c(1.522233+0.34815531i, 1.999903+0.01395469i))) < 1e-6;              c(stable, full)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE");
}

#[test]
fn complex_qr_q_r_and_coef_match_gnu() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "a <- matrix(c(1+1i, 2+1i, 3+2i, 4+2i), 2, 2);              q <- qr(a);              cf <- qr.coef(q, c(1+0i, 1+0i));              c(identical(dim(qr.Q(q)), c(2L, 2L)), identical(dim(qr.R(q)), c(2L, 2L)),                max(abs(cf - c(-0.4+0.2i, 0.4-0.2i))) < 1e-10,                max(abs(qr.Q(q) - matrix(c(-0.5222330-0.3481553i, -0.6963106-0.3481553i,                                          0.7784989-3.910654e-16i, -0.6227992+7.784989e-02i), 2, 2))) < 1e-7,                max(abs(qr.R(q) - matrix(c(-5.744563+0i, 0+0i, -2.6111648-0.1740777i, -0.3892495+0i), 2, 2))) < 1e-6,                max(abs(qr.X(q) - a)) < 1e-8)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE TRUE TRUE");
}

#[test]
fn complex_qr_tall_pivoting_qy_qty_and_coef_match_gnu() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "q3 <- qr(matrix(c(1+1i, 2, 1+2i, 3, 1+1i, 2), 3, 2));              cf3 <- qr.coef(q3, c(1, 1, 1));              c(identical(q3$rank, 2L), identical(q3$pivot, 2:1),                max(abs(cf3 - c(0.2527472527-0.0879120879i, 0.2527472527-0.1098901099i))) < 1e-8,                max(abs(qr.qy(q3, matrix(c(1+1i, 2+2i, 3+3i), 3)) -                          matrix(c(-1.8708259-2.6191183i, -3.3402449+0.0253799i,                                          2.5271797+0.3093734i), 3))) < 1e-6,                max(abs(qr.qty(q3, c(1+1i, 2+2i, 3+3i)) -                          c(-3.3565856-2.3237900i, -2.4089272-0.4601322i, 2.1780672+0.7580926i))) < 1e-6)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE TRUE");
}

#[test]
fn complex_qr_gnu_error_and_argument_semantics() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "q <- qr(matrix(c(1+1i, 2+1i, 3+2i, 4+2i), 2, 2));              errs <- c(grepl('complex matrix', try(qr.qy(q, c(1, 2)), silent=TRUE)[1]),                grepl('not implemented', try(qr.resid(q, c(1+0i, 1+0i)), silent=TRUE)[1]),                grepl('not implemented', try(qr.fitted(q, c(1+0i, 1+0i)), silent=TRUE)[1]));              args <- inherits(try(qr(matrix(1+1i, 2, 2), tol='bogus'), silent=TRUE), 'qr') &&                        identical(qr(matrix(1+1i, 2, 2), LAPACK=TRUE)$qraux[1], 1.5+0.5i);              c(errs, args)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE");
}
