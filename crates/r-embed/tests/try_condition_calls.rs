use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn try_renders_and_preserves_the_original_condition_call() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!(
                    "fixtures/try-condition-call-public-contract.R"
                ))
                .unwrap(),
            "[1] TRUE\n",
            "portable={portable}"
        );
    }
}
