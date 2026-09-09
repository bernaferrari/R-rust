use r_embed::RSession;

fn warning_count(session: &mut RSession, expression: &str) -> String {
    session
        .eval(&format!(
            "n <- 0L; withCallingHandlers({expression}, warning=function(e) {{ n <<- n+1L; invokeRestart('muffleWarning') }}); n"
        ))
        .unwrap()
}

#[test]
fn sequence_recycling_warning_is_once_per_session_not_process() {
    let mut first = RSession::new().unwrap();
    let mut second = RSession::new().unwrap();
    // Explicit recycling and ordinary calls must not consume the warning.
    assert!(warning_count(&mut first, "sequence(2L, from=1:2, recycle=FALSE)").ends_with("[1] 0"));
    assert!(warning_count(&mut first, "sequence(2L)").ends_with("[1] 0"));
    assert!(warning_count(&mut first, "sequence(2L, from=1:2)").ends_with("[1] 1"));
    assert!(warning_count(&mut second, "sequence(2L, from=1:2)").ends_with("[1] 1"));
    assert!(warning_count(&mut first, "sequence(2L, from=1:2)").ends_with("[1] 0"));
    assert!(warning_count(&mut second, "sequence(2L, from=1:2)").ends_with("[1] 0"));
    drop(first);
    drop(second);
    let mut fresh = RSession::new().unwrap();
    assert!(warning_count(&mut fresh, "sequence(2L, from=1:2)").ends_with("[1] 1"));
}
