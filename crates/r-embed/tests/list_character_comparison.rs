use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn list_character_comparisons_and_classed_supsmu_match_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/list-character-contract.R"))
                .unwrap(),
            "list comparison and supsmu: PASS\n",
            "portable={portable}"
        );
    }
}
