use r_embed::{RSession, RuntimePathPolicy};

// Oracle: Homebrew GNU R 4.6.1, optimize.c fcn1/fcn2.
#[test]
fn callback_value_types_and_lengths_match_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/optimizer-scalar-value/controls.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/optimizer-scalar-value/controls.out"),
            "portable={portable}"
        );
    }
}
