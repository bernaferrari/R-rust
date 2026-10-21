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
