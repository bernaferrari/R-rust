use r_embed::RSession;

#[test]
fn loess_na_exclude_restores_omitted_rows() {
    let mut s = RSession::new().unwrap();
    let result = s
        .eval(
            "x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA; f<-loess(y~x,na.action=na.exclude); g<-loess(y~x,na.action=na.omit); p<-predict(f); paste(c(class(f$na.action),paste(as.integer(f$na.action),collapse=','),length(p),paste(which(is.na(p)),collapse=','),is.na(p)[4],is.nan(p)[4],length(predict(g)),all.equal(p[-c(4,17)],predict(g))),collapse=',')",
        )
        .unwrap();
    assert_eq!(result, "[1] \"exclude,4,17,30,4,17,TRUE,FALSE,28,TRUE\"\n");
}

#[test]
fn loess_na_exclude_keeps_explicit_newdata_shape_and_serializes() {
    let mut s = RSession::new().unwrap();
    assert_eq!(
        s.eval(
            "x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA; f<-loess(y~x,na.action=na.exclude); gctorture(TRUE); p<-predict(f,newdata=data.frame(x=c(.2,NA,.8))); q<-predict(f,se=TRUE); g<-unserialize(serialize(f,NULL)); r<-predict(g); gctorture(FALSE); paste(c(length(p),paste(which(is.na(p)),collapse=','),length(q$fit),length(r),paste(which(is.na(r)),collapse=',')),collapse=',')",
        )
        .unwrap(),
        "[1] \"3,2,28,30,4,17\"\n"
    );
}

// Oracle-verified behavior (GNU R, r-oracle bac58395): malformed `exclude`
// na.action metadata is consumed by `napredict`-style index arithmetic, not
// checked admission. GNU only errors on `NA_integer_` positions ("NAs are not
// allowed in subscripted assignments"); out-of-range/duplicate/zero/double
// positions produce recycled-index warnings and GNU-identical output.
#[test]
fn loess_na_exclude_matches_gnu_for_malformed_metadata() {
    let mut s = RSession::new().unwrap();
    s.eval("x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA; f<-loess(y~x,na.action=na.exclude)")
        .unwrap();
    for (code, expected) in [
        // omit = 0 drops nothing from the index vector: everything becomes NA.
        (
            "f$na.action<-structure(c(0L),class='exclude');p<-predict(f)",
            "29;29;1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29",
        ),
        // omit = 100 recycles `1L:n` into an empty selection: values survive, none NA.
        (
            "f$na.action<-structure(c(100L),class='exclude');p<-predict(f)",
            "29;0;",
        ),
        // duplicate omit = c(2,2) extends the result to 30 with one NA at 2.
        (
            "f$na.action<-structure(c(2L,2L),class='exclude');p<-predict(f)",
            "30;1;2",
        ),
        // double-typed position is coerced and treated like the integer case.
        (
            "f$na.action<-structure(c(2),class='exclude');p<-predict(f)",
            "29;1;2",
        ),
    ] {
        let summary = s
            .eval(&format!(
                "{code};paste(c(length(p),sum(is.na(p)),paste(which(is.na(p)),collapse=',')),collapse=';')"
            ))
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        // The port, like GNU, prints recycled-index warnings for some cases;
        // match the value line regardless of any preceding warning text.
        assert!(
            summary.trim_end().ends_with(&format!("[1] \"{expected}\"")),
            "{code}: {summary}"
        );
        assert_eq!(s.eval("2+2").unwrap(), "[1] 4\n");
    }
    // NA positions are the one malformed case GNU errors on — the port must
    // surface a recoverable error and leave the session usable.
    assert!(
        s.eval("f$na.action<-structure(c(NA_integer_),class='exclude');predict(f)")
            .is_err()
    );
    assert_eq!(s.eval("2+2").unwrap(), "[1] 4\n");
}
