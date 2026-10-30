use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn pairlist_names_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(include_str!("fixtures/names-pairlist/controls.R"))
            .unwrap();
        let expected = include_str!("fixtures/names-pairlist/controls.out");
        for (actual, expected) in output.lines().zip(expected.lines()) {
            assert_eq!(actual, expected, "portable={portable}");
        }
        assert_eq!(
            output.lines().count(),
            expected.lines().count(),
            "portable={portable}"
        );
    }
}
