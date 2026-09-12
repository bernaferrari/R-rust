use r_embed::RSession;

#[test]
fn qr_x_reconstructs_linpack_and_lapack_matrix() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            "x <- matrix(c(1,2,3,4,5,6), 3, 2); \
             q0 <- qr(x); q1 <- qr(x, LAPACK=TRUE); \
             c(max(abs(qr.X(q0)-x))<1e-12, max(abs(qr.X(q1)-x))<1e-12)",
        )
        .unwrap();
    assert_eq!(result.trim(), "[1] TRUE TRUE");
}

#[test]
fn qr_x_rejects_non_qr_and_recovers() {
    let mut session = RSession::new().unwrap();
    let error = session.eval("qr.X(1:3)").unwrap_err();
    assert!(error.to_string().contains("QR decomposition"), "{error}");
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
