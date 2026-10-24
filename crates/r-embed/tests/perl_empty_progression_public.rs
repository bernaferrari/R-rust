use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn perl_empty_pattern_positions_and_lengths_match_gnu_for_utf8_subjects() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/perl-empty-pattern-contract.R"))
            .unwrap(),
        include_str!("fixtures/perl-empty-pattern-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn gregexpr_position_tables_obey_original_native_budget_and_recover() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session.eval("subject<-strrep('a',100000L);invisible(gregexpr('', 'a', perl=TRUE));gc();invisible(NULL)").unwrap();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 1024 * 1024;
    session.set_resource_limits(limits).unwrap();
    let error = session
        .eval("invisible(gregexpr('',subject,perl=TRUE))")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("regex workspace exceeds session memory limit"),
        "{error}"
    );
    assert_eq!(
        session.resource_limits().max_alloc_bytes,
        limits.max_alloc_bytes
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
    assert_eq!(session.eval("nchar(subject)").unwrap(), "[1] 100000\n");
    assert_eq!(
        session
            .eval("as.integer(gregexpr('', 'a', perl=TRUE)[[1L]])")
            .unwrap(),
        "[1] 1\n"
    );
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
}
