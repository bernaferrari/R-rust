use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn complex_literals_in_operators_match_pinned_gnu() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp"))
        } else {
            RSession::new()
        }
        .unwrap();
        if portable {
            session.enable_browser_files();
        }
        let output = session
            .eval(include_str!("fixtures/deparse-complex-precedence.R"))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deparse-complex-precedence.out"),
            "portable={portable}"
        );
    }
}
