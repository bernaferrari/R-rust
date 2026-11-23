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

#[test]
fn nullable_tre_captures_preserve_positions_and_lengths() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(
                r#"
local({
    x <- c("", "a", "b", "ab", "aab", "bb")
    m <- regexec("^(a*)(b?)$", x)
    stopifnot(identical(lapply(m, as.integer),
                       list(c(1L,1L,1L),c(1L,1L,2L),c(1L,1L,1L),c(1L,1L,2L),c(1L,1L,3L),-1L)),
              identical(lapply(m, attr, "match.length"),
                       list(c(0L,0L,0L),c(1L,1L,0L),c(1L,0L,1L),c(2L,1L,1L),c(3L,2L,1L),-1L)))
    cat("nullable-tre-ok\n")
})
"#
            )
            .unwrap(),
        "nullable-tre-ok\n"
    );
}

#[test]
fn nested_tre_alternations_preserve_capture_order() {
    let mut session = RSession::new().unwrap();
    // Keep rport's existing -1/-1 for groups in an unvisited alternative.
    // GNU R reports 0/0 for those groups.
    assert_eq!(
        session
            .eval(
                r#"local({
    m <- regexec('^((a(b)?)|((c)?d?))$', c('a','ab','c','d','cd','ac'))
    stopifnot(identical(lapply(m, as.integer),
        list(c(1L,1L,1L,-1L,-1L,-1L), c(1L,1L,1L,2L,-1L,-1L), c(1L,1L,-1L,-1L,1L,1L),
             c(1L,1L,-1L,-1L,1L,-1L), c(1L,1L,-1L,-1L,1L,1L), -1L)),
        identical(lapply(m, attr, 'match.length'),
        list(c(1L,1L,1L,-1L,-1L,-1L), c(2L,2L,2L,1L,-1L,-1L), c(1L,1L,-1L,-1L,1L,1L),
             c(1L,1L,-1L,-1L,1L,-1L), c(2L,2L,-1L,-1L,2L,1L), -1L)))
    cat('capture-order-ok\n')
})"#
            )
            .unwrap(),
        "capture-order-ok\n"
    );
}
