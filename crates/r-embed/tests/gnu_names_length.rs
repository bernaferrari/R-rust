use r_embed::{RSession, RuntimePathPolicy};

#[test]
#[ignore = "rport-6oxoy: names length normalization is not yet implemented"]
fn vector_names_lengths_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session.eval(include_str!("fixtures/names-length/controls.R")).unwrap();
        assert_eq!(actual, include_str!("fixtures/names-length/controls.out"), "portable={portable}");
    }
}
