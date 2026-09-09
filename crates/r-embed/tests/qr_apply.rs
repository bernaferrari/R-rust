use r_embed::RSession;
#[test]
fn qr_qy_qty_match_gnu_and_preserve_matrix_names() {
    let mut s = RSession::new().unwrap();
    let z=s.eval("local({x<-matrix(1:6,3,2);q<-qr(x);y<-matrix(1:6,3,2,dimnames=list(c('u','v','w'),c('p','q')));a<-qr.qy(q,y);b<-qr.qty(q,y);c(max(abs(a-qr.Q(q,complete=TRUE)%*%y))<1e-8,max(abs(b-t(qr.Q(q,complete=TRUE))%*%y))<1e-8,identical(dimnames(a),dimnames(y)),identical(dimnames(b),dimnames(y)))})").unwrap();
    assert_eq!(z.trim(), "[1] TRUE TRUE TRUE TRUE");
}
#[test]
fn qr_apply_rank_deficient_lapack_and_empty_recover() {
    let mut s = RSession::new().unwrap();
    let z=s.eval("local({x<-matrix(c(1,2,3,2,4,6),3,2);y<-1:3;q<-qr(x);l<-qr(x,LAPACK=TRUE);c(max(abs(qr.qy(q,y)-qr.Q(q,complete=TRUE)%*%y))<1e-8,max(abs(qr.qy(l,y)-qr.Q(l,complete=TRUE)%*%y))<1e-8,identical(dim(qr.qy(q,matrix(numeric(),3,0))),c(3L,0L)) )})").unwrap();
    assert_eq!(z.trim(), "[1] TRUE TRUE TRUE");
    let e = s
        .eval("qr.qy(qr(matrix(1:6,3,2)),1:2)")
        .expect_err("bad rows");
    assert!(e.to_string().contains("rows"));
    assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
}

#[test]
fn qr_apply_named_reordered_args_and_rhs_coercions_match_gnu() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({q<-qr(matrix(1:6,3,2)); y<-1:3; names(y)<-c('u','v','w'); a<-qr.qy(y=y,qr=q); b<-qr.qty(qr=q,y=as.logical(c(1,0,1))); c(identical(names(a),names(y)),max(abs(a-c(2.7032268,-2.5475764,-.4499104)))<1e-6, length(b)==3L)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE TRUE TRUE");

    let got = s.eval("local({q<-qr(matrix(1:6,3,2)); c(max(abs(qr.qy(q,c('1','2','3'))-qr.qy(q,1:3)))<1e-8, max(abs(qr.qy(q,c(TRUE,FALSE,TRUE))-qr.qy(q,c(1,0,1))))<1e-8)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE TRUE");
}

#[test]
fn qr_apply_lapack_vector_rhs_is_one_column_matrix_with_dimnames() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({q<-qr(matrix(1:6,3,2),LAPACK=TRUE); y<-1:3; z<-qr.qy(q,y); m<-matrix(1:3,3,1,dimnames=list(c('r1','r2','r3'),'z')); w<-qr.qy(qr=q,y=m); c(identical(dim(z),c(3L,1L)),is.null(dimnames(z)),identical(dimnames(w),dimnames(m)))})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE TRUE TRUE");
}

#[test]
fn qr_apply_rejects_short_qraux_and_bad_rhs_dimensions_recoverably() {
    let mut s = RSession::new().unwrap();
    let error = s
        .eval("q<-qr(matrix(1:6,3,2)); q$qraux<-numeric(); qr.qy(q,1:3)")
        .expect_err("short qraux must fail");
    assert!(error.to_string().contains("QR"));
    let error = s
        .eval("qr.qty(qr=qr(matrix(1:6,3,2)), y=matrix(1:2,2,1))")
        .expect_err("bad rhs rows must fail");
    assert!(error.to_string().contains("rows"));
    assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
}

#[test]
fn qr_apply_preserves_coerced_arguments_during_collection() {
    let mut s = RSession::new().unwrap();
    let result = s.eval("local({q<-qr(matrix(1:6,3,2),LAPACK=TRUE);gctorture(TRUE);on.exit(gctorture(FALSE)); y<-c('1','2','3');z<-qr.qty(y=qr.qy(y=y,qr=q),qr=q);identical(dim(z),c(3L,1L)) && max(abs(z-1:3))<1e-10})").unwrap();
    assert_eq!(result.trim(), "[1] TRUE");
}

#[test]
fn qr_apply_rejects_nonfinite_linpack_inputs_and_recovers() {
    let mut s = RSession::new().unwrap();
    for code in [
        "qr.qy(qr(matrix(1:6,3,2)),c(1,NaN,3))",
        "q<-qr(matrix(1:6,3,2));q$qraux[1]<-Inf;qr.qty(q,1:3)",
        "q<-qr(matrix(1:6,3,2));q$qr[1,1]<-NA_real_;qr.qy(q,1:3)",
    ] {
        let error = s.eval(code).expect_err("nonfinite LINPACK input");
        assert!(error.to_string().contains("NA/NaN/Inf"), "{error}");
        assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
    }
}

#[test]
fn qr_apply_allocation_limit_is_recoverable() {
    let mut s = RSession::new().unwrap();
    s.eval("q<-qr(matrix(1:6,3,2));y<-matrix(1,3,1000)")
        .unwrap();
    let mut limits = s.resource_limits();
    limits.max_alloc_bytes = 1;
    s.set_resource_limits(limits).unwrap();
    let error = s.eval("qr.qy(q,y)").expect_err("bounded output allocation");
    assert!(error.to_string().contains("allocation"), "{error}");
    limits.max_alloc_bytes = 0;
    s.set_resource_limits(limits).unwrap();
    assert_eq!(s.eval("1+1").unwrap(), "[1] 2");
}
