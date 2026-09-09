use r_embed::RSession;

#[test]
fn qr_q_matches_gnu_and_reconstructs_tall_default_and_lapack() {
    let mut s = RSession::new().unwrap();
    let got=s.eval("local({x<-matrix(1:6,3,2); q<-qr(x); Q<-qr.Q(q); lap<-qr(x,LAPACK=TRUE); ql<-qr.Q(lap); c(identical(dim(Q),c(3L,2L)),max(abs(Q-c(-.2672612419,-.5345224838,-.8017837257,.8728715609,.2182178902,-.4364357805)))<1e-9, max(abs(Q%*%qr.R(q)-x))<1e-8, max(abs(ql%*%qr.R(lap)-x[,lap$pivot]))<1e-8)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE TRUE TRUE TRUE");
}

#[test]
fn qr_q_complete_wide_empty_and_dvec_contracts() {
    let mut s = RSession::new().unwrap();
    let got=s.eval("local({w<-matrix(1:8,2,4); e<-qr.Q(qr(matrix(numeric(),0,2))); q<-qr.Q(qr(matrix(1:6,3,2)),Dvec=c(1,0)); c(identical(dim(qr.Q(qr(w),complete=TRUE)),c(2L,2L)),identical(dim(e),c(0L,0L)),max(abs(q[,2]))==0)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE TRUE TRUE");
    let err = s.eval("qr.Q(list())").expect_err("invalid qr");
    assert!(err.to_string().contains("QR"));
    assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
}

#[test]
fn qr_q_named_arguments_survive_gctorture() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({x<-matrix(1:6,3,2);q<-qr(x);gctorture(TRUE);on.exit(gctorture(FALSE));z<-qr.Q(complete=FALSE,qr=q,Dvec=c(1,0)); identical(dim(z),c(3L,2L)) && max(abs(z[,2]))==0})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE");
}

#[test]
fn qr_q_rank_deficient_reconstructs_and_complete_is_orthogonal() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({x<-matrix(c(1,2,3,2,4,6),3,2);q<-qr(x);Q<-qr.Q(q,complete=TRUE);c(max(abs(qr.Q(q)%*%qr.R(q)-x))<1e-8,max(abs(crossprod(Q)-diag(3)))<1e-8)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE TRUE");
}

#[test]
fn qr_q_rejects_malformed_aux_and_recovers() {
    let mut s = RSession::new().unwrap();
    let error = s
        .eval("q<-qr(matrix(1:6,3,2)); q$qraux<-numeric(); qr.Q(q)")
        .expect_err("malformed qraux must fail");
    assert!(error.to_string().contains("QR"));
    assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
}

#[test]
fn qr_q_allocation_limit_is_recoverable() {
    let mut s = RSession::new().unwrap();
    s.eval("q<-qr(matrix(1,2000,80))").unwrap();
    let mut limits = s.resource_limits();
    limits.max_alloc_bytes = 1;
    s.set_resource_limits(limits).unwrap();
    let error = s
        .eval("qr.Q(q,complete=TRUE)")
        .expect_err("Q allocation must be limited");
    assert!(error.to_string().contains("allocation"), "{error}");
    limits.max_alloc_bytes = 0;
    s.set_resource_limits(limits).unwrap();
    assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
}
