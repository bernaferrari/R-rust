use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn match_call_keeps_forwarded_dots_references_without_forcing() {
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
                    "fixtures/match-call-forwarded-public-contract.R"
                ))
                .unwrap(),
            "[1] TRUE\n",
            "portable={portable}"
        );
    }
}
