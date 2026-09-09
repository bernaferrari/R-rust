use r_embed::RSession;

#[test]
fn primitive_dispatch_preserves_the_first_argument_expression() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("n<-2L;identical(rep(1L,n+1L),c(1L,1L,1L))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    for expression in [
        "rep.foo<-function(x,...) deparse(substitute(x));x<-structure(1,class='foo');rep(x,2)",
        "`[.foo`<-function(x,...) deparse(substitute(x));x<-structure(1,class='foo');x[1]",
    ] {
        let result = session.eval(expression).unwrap();
        assert_eq!(result.trim(), "[1] \"x\"");
    }
}

#[test]
fn rep_method_evaluates_first_argument_once_and_keeps_dots_lazy() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("calls<-0L;rep.foo<-function(x,...)42L;x<-structure(1,class='foo');value<-rep({calls<-calls+1L;x},stop('unused'));identical(value,42L)&&identical(calls,1L)").unwrap().trim(), "[1] TRUE");
    assert_eq!(session.eval("local({secret<-42L;rep.foo<-function(x,...)get('secret',parent.frame());identical(rep(structure(1,class='foo')),42L)})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn primitive_default_evaluation_preserves_missing_and_forwarded_arguments() {
    let mut session = RSession::new().unwrap();
    for expression in [
        "identical(rep(1L,,length.out=3L),c(1L,1L,1L))",
        "f<-function(...)rep(1L,...);identical(f(times=3L),c(1L,1L,1L))",
        "d<-data.frame(x=1:3,y=4:6);identical(d[2,]$y,5L)",
    ] {
        assert_eq!(
            session.eval(expression).unwrap().trim(),
            "[1] TRUE",
            "{expression}"
        );
    }
}
