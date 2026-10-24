use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn grep_indices_values_and_names_preserve_original_utf8_contract() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/grep-owned-index-contract.R"))
            .unwrap(),
        include_str!("fixtures/grep-owned-index-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn grep_indices_obey_original_cap_and_recover_without_raising_it() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    session
        .eval("subject<-rep('é',100000L);invisible(grep('é','é',fixed=TRUE));gc();invisible(NULL)")
        .unwrap();
    let before = session.memory_usage().unwrap();
    let mut limits = session.resource_limits();
    limits.max_alloc_bytes = before.total_bytes + 1024 * 1024;
    session.set_resource_limits(limits).unwrap();
    let error = session
        .eval("invisible(grep('é',subject,fixed=TRUE))")
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
    assert_eq!(session.eval("length(subject)").unwrap(), "[1] 100000\n");
    assert_eq!(
        session.eval("grep('é',subject[1L],fixed=TRUE)").unwrap(),
        "[1] 1\n"
    );
    assert_eq!(
        session.memory_usage().unwrap().workspace_bytes,
        before.workspace_bytes
    );
}
