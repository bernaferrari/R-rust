use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn deriv_hessian_flags_match_pinned_gnu_coercion() {
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
                "fixtures/deriv-hessian-coercion-public-contract.R"
            ))
            .unwrap();
        assert_eq!(
            output,
            include_str!("fixtures/deriv-hessian-coercion-public-contract.out"),
            "portable={portable}"
        );
    }
}
