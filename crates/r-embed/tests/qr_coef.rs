use r_embed::RSession;

#[test]
fn qr_coef_linpack_matches_gnu_two_column_example() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "x <- matrix(c(1,2,3,4,5,6), 3, 2); \
             q <- qr(x); \
             c <- qr.coef(q, c(1,2,3)); \
             c(length(c)==2L, abs(c[1]-1)<1e-10, abs(c[2])<1e-10)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE");
}

#[test]
fn qr_coef_lapack_matches_gnu_two_column_example() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "x <- matrix(c(1,2,3,4,5,6), 3, 2); \
             q <- qr(x, LAPACK=TRUE); \
             c <- qr.coef(q, c(1,2,3)); \
             c(length(c)==2L, abs(c[1]-1)<1e-10, abs(c[2])<1e-10)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE TRUE");
}

#[test]
fn qr_coef_rejects_row_mismatch_and_recovers() {
    let mut session = RSession::new().unwrap();
    let error = session
        .eval("qr.coef(qr(matrix(1:6,3,2)), 1:2)")
        .unwrap_err();
    assert!(error.to_string().contains("same number of rows"), "{error}");
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
