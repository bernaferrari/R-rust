use r_embed::RSession;

#[test]
fn named_s4_signature_follows_generic_argument_order() {
    // Pinned GNU R: mix(1, "a") returns "matched", even with reversed names.
    let mut session = RSession::new().unwrap();
    let result = session.eval("setGeneric('mix',function(x,y) standardGeneric('mix')); setMethod('mix',c(y='character',x='numeric'),function(x,y) 'matched'); mix(1,'a')").unwrap();
    assert!(result.contains("matched"), "{}", result);
}

#[test]
fn invalid_named_signature_does_not_register_a_method() {
    let mut session = RSession::new().unwrap();
    session
        .eval("setGeneric('mix',function(x,y) standardGeneric('mix'))")
        .unwrap();
    assert!(
        session
            .eval("setMethod('mix',c(z='numeric'),function(x,y) 'wrong')")
            .is_err()
    );
    let result = session
        .eval("setMethod('mix',c(x='numeric',y='character'),function(x,y) 'ok'); mix(1,'a')")
        .unwrap();
    assert!(result.contains("ok"));
}

#[test]
fn separate_generics_do_not_share_method_tables_or_lose_lexical_captures() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("setGeneric('aa',function(x) standardGeneric('aa')); setMethod('aa','numeric',function(x) 'a'); setGeneric('bb',function(x) standardGeneric('bb')); setMethod('bb','numeric',function(x) 'b'); cat(aa(1),bb(1))").unwrap();
    assert_eq!(result.trim(), "a b");
    let captured = session.eval("make <- function() { suffix <- '!'; setGeneric('cc',function(x) paste0(standardGeneric('cc'),suffix)); setMethod('cc','numeric',function(x) 'c'); cc }; cc <- make(); cat(cc(1))").unwrap();
    assert_eq!(captured.trim(), "c!");
}

#[test]
fn generic_setup_and_dispatch_survive_forced_collection() {
    let mut session = RSession::new().unwrap();
    let output = session.eval("gctorture(TRUE); setGeneric('safeGeneric',function(x) standardGeneric('safeGeneric')); setMethod('safeGeneric','numeric',function(x) x+1); answer <- safeGeneric(4); gctorture(FALSE); cat(answer)").unwrap();
    assert_eq!(output.trim(), "5");
}
