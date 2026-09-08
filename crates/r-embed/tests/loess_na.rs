use r_embed::RSession;

#[test]
fn loess_na_exclude_restores_omitted_rows() {
    let mut s = RSession::new().unwrap();
    let result = s
        .eval(
            "x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA; f<-loess(y~x,na.action=na.exclude); g<-loess(y~x,na.action=na.omit); p<-predict(f); paste(c(class(f$na.action),paste(as.integer(f$na.action),collapse=','),length(p),paste(which(is.na(p)),collapse=','),is.na(p)[4],is.nan(p)[4],length(predict(g)),all.equal(p[-c(4,17)],predict(g))),collapse=',')",
        )
        .unwrap();
    assert_eq!(result, "[1] \"exclude,4,17,30,4,17,TRUE,FALSE,28,TRUE\"");
}

#[test]
fn loess_na_exclude_keeps_explicit_newdata_shape_and_serializes() {
    let mut s = RSession::new().unwrap();
    assert_eq!(
        s.eval(
            "x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA; f<-loess(y~x,na.action=na.exclude); gctorture(TRUE); p<-predict(f,newdata=data.frame(x=c(.2,NA,.8))); q<-predict(f,se=TRUE); g<-unserialize(serialize(f,NULL)); r<-predict(g); gctorture(FALSE); paste(c(length(p),paste(which(is.na(p)),collapse=','),length(q$fit),length(r),paste(which(is.na(r)),collapse=',')),collapse=',')",
        )
        .unwrap(),
        "[1] \"3,2,28,30,4,17\""
    );
}

#[test]
fn loess_na_exclude_rejects_malformed_metadata_without_poisoning_session() {
    let mut s = RSession::new().unwrap();
    s.eval("x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA; f<-loess(y~x,na.action=na.exclude)")
        .unwrap();
    for code in [
        "f$na.action<-structure(c(0L),class='exclude');predict(f)",
        "f$na.action<-structure(c(100L),class='exclude');predict(f)",
        "f$na.action<-structure(c(2L,2L),class='exclude');predict(f)",
        "f$na.action<-structure(c(2),class='exclude');predict(f)",
        "f$na.action<-structure(c(NA_integer_),class='exclude');predict(f)",
    ] {
        assert!(s.eval(code).is_err(), "{code}");
        assert_eq!(s.eval("2+2").unwrap(), "[1] 4");
    }
}
