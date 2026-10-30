use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn direct_classed_names_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(include_str!("fixtures/names-classed-attribute/controls.R"))
            .unwrap();
        let expected = include_str!("fixtures/names-classed-attribute/controls.out");
        let actual_lines: Vec<_> = output.lines().collect();
        let expected_lines: Vec<_> = expected.lines().collect();
        assert_eq!(
            actual_lines.len(),
            expected_lines.len(),
            "portable={portable}"
        );
        for (actual, expected) in actual_lines.iter().zip(&expected_lines) {
            assert_eq!(actual, expected, "portable={portable}");
        }
    }
}
