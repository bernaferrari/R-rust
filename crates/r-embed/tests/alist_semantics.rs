// Oracle: GNU R bac583951b728e97b9786804d3b4081f0fe18df5; rport-zha9.
use r_embed::RSession;

#[test]
fn alist_keeps_calls_symbols_and_missing_arguments_unevaluated() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({x<-99; a<-alist(1+1,z=3,,q=foo); identical(typeof(a),'list') && length(a)==4L && identical(a[[1]],quote(1+1)) && identical(a[[4]],quote(foo)) && is.symbol(a[[3]]) && identical(a[[2]],3)})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE");
}

#[test]
fn alist_preserves_names_and_empty_result() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({a<-alist(1+1,z=3,,q=foo); identical(names(a),c('', 'z', '', 'q')) && identical(alist(),list())})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE");
}

#[test]
fn alist_does_not_force_erroring_expression_and_recovers() {
    let mut s = RSession::new().unwrap();
    let got = s
        .eval("local({a<-alist(stop('must not run'), answer=2); c(length(a),a$answer)})")
        .unwrap();
    assert_eq!(got.trim(), "[1] 2 2");
    assert_eq!(s.eval("1+1").unwrap().trim(), "[1] 2");
}

#[test]
fn alist_is_a_closure_and_preserves_forwarded_dots_syntax() {
    let mut s = RSession::new().unwrap();
    let got = s.eval("local({f<-function(x,...)alist(x,...); a<-f(stop('unforced'),z=2); identical(typeof(alist),'closure') && identical(a,list(quote(x),quote(...)))})").unwrap();
    assert_eq!(got.trim(), "[1] TRUE");
}

#[test]
fn alist_body_matches_gnu_source() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(body(alist),quote(as.list(sys.call())[-1L]))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
