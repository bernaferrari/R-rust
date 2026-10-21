use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn simple_condition_ownership_matches_gnu_bytes_vectors_methods_and_recovery() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!(
                "fixtures/simple-condition-ownership-contract.R"
            ))
            .unwrap(),
        include_str!("fixtures/simple-condition-ownership-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}
