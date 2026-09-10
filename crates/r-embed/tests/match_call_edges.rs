use r_embed::RSession;

#[test]
fn match_call_rejects_ambiguous_and_duplicate_named_arguments() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "isTRUE(tryCatch(match.call(definition=function(alpha,alpine){},call=quote(f(al=1))),error=function(e)TRUE))&&isTRUE(tryCatch(match.call(definition=function(alpha){},call=quote(f(alpha=1,alpha=2))),error=function(e)TRUE))",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_rejects_positional_arguments_after_dots() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "identical(match.call(definition=function(a,...,b){},call=quote(f(1,2))),quote(f(a=1,2)))",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_expand_dots_preserves_a_pairlist() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "f<-function(...)match.call(expand.dots=FALSE);mc<-f(1+1,z=3);identical(typeof(mc[[2]]),'pairlist')&&identical(as.list(mc[[2]]),list(quote(1+1),z=3))",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_does_not_evaluate_argument_expressions() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "i<-0L;f<-function(a,b){match.call();i<<-i+1L;i};identical(f({i<<-i+1L;1L},{i<<-i+1L;2L}),1L)",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_preserves_source_under_gctorture() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "q<-quote(function(first,second)match.call());before<-serialize(q,NULL);gctorture(TRUE);fn<-eval(q);invisible(fn(first=1,second=2));gctorture(FALSE);identical(before,serialize(q,NULL))",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_forwarded_dots_retain_lazy_references() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("f<-function(...)match.call();g<-function(...)f(...);identical(g(x=1+2,y=4),quote(f(x=..1,y=4)))").unwrap().trim(), "[1] TRUE");
}
