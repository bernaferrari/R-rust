use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn public_as_vector_mode_matches_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let output = session
            .eval(include_str!("fixtures/as-vector-mode/controls.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/as-vector-mode/controls.out"),
            "portable={portable}"
        );
    }
}
