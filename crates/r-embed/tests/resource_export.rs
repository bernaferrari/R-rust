use r_embed::RSession;

#[test]
fn bounded_results_reject_before_copy_and_preserve_session() {
    let mut session = RSession::new().unwrap();
    session.set_result_limit(Some(4096));
    session
        .eval("x <- rep('abcdef', 1000)")
        .expect("invisible assignment needs no export");
    let error = session.eval("x").unwrap_err().to_string();
    assert!(error.contains("export budget"), "{error}");
    assert!(
        session
            .eval_result("x")
            .unwrap_err()
            .to_string()
            .contains("export budget")
    );
    assert_eq!(session.eval("length(x)").unwrap(), "[1] 1000");
    assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2");
}

#[test]
fn repeated_references_and_deep_lists_cannot_expand_without_bound() {
    let mut session = RSession::new().unwrap();
    session.set_result_limit(Some(4096));
    session
        .eval("x <- rep('a', 10); y <- rep(list(x), 100)")
        .unwrap();
    assert!(
        session
            .eval_result("y")
            .unwrap_err()
            .to_string()
            .contains("export budget")
    );
    session
        .eval("deep <- NULL; for (i in 1:100) deep <- list(deep)")
        .unwrap();
    assert!(session.eval_result("deep").is_err());
    assert_eq!(session.eval("length(deep)").unwrap(), "[1] 1");
}

#[test]
fn native_hosts_keep_unlimited_default_and_session_local_settings() {
    let mut limited = RSession::new().unwrap();
    let mut ordinary = RSession::new().unwrap();
    limited.set_result_limit(Some(512));
    assert!(limited.eval_result("seq_len(100)").is_err());
    assert!(ordinary.eval_result("seq_len(100)").is_ok());
    limited.set_result_limit(None);
    assert!(limited.eval_result("seq_len(100)").is_ok());
}

#[test]
fn exhausted_node_budget_never_reports_a_successful_null_result() {
    let mut session = RSession::new().unwrap();
    let mut limits = session.resource_limits();
    limits.max_arena_nodes = session.arena_stats().active_nodes + 100;
    session.set_resource_limits(limits).unwrap();
    assert!(session.eval("for (i in 1:1000) new.env()").is_err());
    session.close();
    let mut fresh = RSession::new().unwrap();
    assert_eq!(fresh.eval("1+1").unwrap(), "[1] 2");
}
