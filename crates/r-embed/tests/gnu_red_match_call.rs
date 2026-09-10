//! Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 evaluates both to TRUE.
//! Beads rport-dat7: normal assertions, no ignored or expected-panic tests.
use r_embed::RSession;

#[test]
fn match_call_matches_partial_and_positional_arguments_to_formals() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("local({f<-function(alpha,beta=2)match.call();identical(f(be=4,1),quote(f(alpha=1,beta=4)))})").unwrap().trim(), "[1] TRUE");
}

#[test]
fn match_call_honors_explicit_definition_and_call() {
    let mut s = RSession::new().unwrap();
    assert_eq!(s.eval("identical(match.call(definition=function(alpha,beta=2){},call=quote(f(be=4,1))),quote(f(alpha=1,beta=4)))").unwrap().trim(), "[1] TRUE");
}
