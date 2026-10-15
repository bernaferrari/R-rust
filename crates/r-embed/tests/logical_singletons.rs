use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn shared_logical_results_duplicate_before_r_level_mutation() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!(
                "fixtures/logical-singletons-public-contract.R"
            ))
            .unwrap(),
        "[1] TRUE\n"
    );
}
