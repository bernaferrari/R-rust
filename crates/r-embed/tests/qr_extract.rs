use r_embed::RSession;

#[test]
fn qr_r_default_trims_rows_and_preserves_dimnames() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            "x <- matrix(1:6, 3, 2, dimnames=list(c('r1','r2','r3'), c('a','b'))); \
             z <- qr.R(qr(x)); \
             c(identical(dim(z), c(2L,2L)), identical(dimnames(z), list(c('r1','r2'),c('a','b'))), \
               max(abs(z - matrix(c(-3.74165738677,0,-8.5523597412,1.9639610121),2,2))) < 1e-9)",
        )
        .expect("qr.R default");
    assert_eq!(result, "[1] TRUE TRUE TRUE");
}

#[test]
fn qr_r_complete_keeps_all_rows_and_recovers_after_invalid_object() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval(
            "x <- matrix(1:6, 3, 2, dimnames=list(c('r1','r2','r3'), c('a','b'))); \
             z <- qr.R(complete=TRUE, qr=structure(list(qr=qr(x)$qr), class='qr')); \
             c(identical(dim(z), c(3L,2L)), identical(dimnames(z), dimnames(x)), \
               z[3,1] == 0, z[3,2] == 0)",
        )
        .expect("qr.R complete");
    assert_eq!(result, "[1] TRUE TRUE TRUE TRUE");

    let error = session.eval("qr.R(list())").expect_err("invalid qr object");
    assert!(error.to_string().contains("QR"));
    assert_eq!(session.eval("1 + 1").expect("session recovery"), "[1] 2");
}

#[test]
fn qr_r_preserves_names_under_gctorture() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("local({x<-matrix(1:6,3,2,dimnames=list(c('a','b','c'),c('x','y')));q<-qr(x);gctorture(TRUE);on.exit(gctorture(FALSE));identical(dimnames(qr.R(q)),list(c('a','b'),c('x','y')))})").unwrap().trim(), "[1] TRUE");
}
