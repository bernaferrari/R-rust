use r_embed::RSession;

#[test]
fn qr_fitted_resid_linpack_match_gnu_two_column_example() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("x <- matrix(c(1,2,3,4,5,6), 3, 2); q <- qr(x); y <- c(1,2,3); f <- qr.fitted(q, y); r <- qr.resid(q, y); c(length(f)==3L, length(r)==3L, max(abs(f-y))<1e-12, max(abs(r))<1e-12)")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE");
}

#[test]
fn qr_fitted_resid_handle_matrix_y() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval("x <- matrix(c(1,2,3,4,5,6), 3, 2); q <- qr(x); ym <- x; fm <- qr.fitted(q, ym); rm <- qr.resid(q, ym); c(identical(dim(fm), c(3L,2L)), identical(dim(rm), c(3L,2L)), max(abs(fm-ym))<1e-12, max(abs(rm))<1e-12)")
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE TRUE");
}

#[test]
fn qr_fitted_resid_reject_row_mismatch_and_lapack() {
    let mut session = RSession::new().unwrap();
    let error = session.eval("qr.fitted(qr(matrix(1:6,3,2)), 1:2)").unwrap_err();
    assert!(error.to_string().contains("same number of rows"), "{error}");
    let error = session.eval("qr.fitted(qr(matrix(1:6,3,2), LAPACK=TRUE), 1:3)").unwrap_err();
    assert!(error.to_string().contains("LAPACK"), "{error}");
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
