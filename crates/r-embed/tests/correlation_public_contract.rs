use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn correlation_public_workflows_match_gnu_under_both_package_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let actual = session
            .eval(include_str!("fixtures/correlation-public-contract.R"))
            .unwrap();
        assert_eq!(
            actual,
            include_str!("fixtures/correlation-public-contract.out"),
            "portable={portable}"
        );
        assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2\n");
    }
}
