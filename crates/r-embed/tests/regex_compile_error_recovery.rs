use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn invalid_tre_patterns_return_errors_and_preserve_the_session() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!(
                "fixtures/regex-compile-error-recovery-contract.R"
            ))
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
}

#[test]
fn tre_compile_memory_denial_is_an_error_and_recovers_under_unchanged_cap() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session
        .eval("pattern<-strrep('a',20000L);invisible(grepl('a','a'));gc();invisible(NULL)")
        .unwrap();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 1024 * 1024;
    session.set_resource_limits(limits).unwrap();
    for code in [
        "grepl(pattern,'a')",
        "regexpr(pattern,'a')",
        "gregexpr(pattern,'a')",
        "sub(pattern,'b','a')",
    ] {
        let error = session.eval(code).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("regex workspace exceeds session memory limit"),
            "{code}: {error}"
        );
        assert_eq!(
            session.resource_limits().max_alloc_bytes,
            limits.max_alloc_bytes
        );
        drop(error);
        // The public error renderer intentionally retains its admitted message.
        // Clear that cache before checking compiler workspace cleanup.
        assert_eq!(
            session
                .eval(".Internal(seterrmessage(''));grepl('a','a')")
                .unwrap(),
            "[1] TRUE\n"
        );
        assert_eq!(
            session.memory_usage().unwrap().workspace_bytes,
            before.workspace_bytes
        );
    }
}
