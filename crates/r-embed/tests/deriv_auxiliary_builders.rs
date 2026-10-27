use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn deriv_auxiliary_builders_match_pinned_gnu_shapes_values_and_closures() {
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
            .eval(include_str!(
                "fixtures/deriv-auxiliary-builders-public-contract.R"
            ))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-auxiliary-builders-public-contract.out"),
            "portable={portable}"
        );
    }
}
