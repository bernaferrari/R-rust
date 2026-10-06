use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn palette_public_workflows_match_gnu_under_both_package_policies() {
    for portable in [false, true] {
        for generation in 0..2 {
            let mut session = if portable {
                RSession::new_with_path_policy(RuntimePathPolicy::new(
                    Vec::new(),
                    std::env::temp_dir(),
                ))
            } else {
                RSession::new()
            }
            .unwrap();
            assert_eq!(
                session
                    .eval(include_str!("fixtures/tempfile-public-contract.R"))
                    .unwrap(),
                include_str!("fixtures/tempfile-public-contract.out"),
                "portable={portable}, generation={generation}"
            );
            let actual = session
                .eval(include_str!("fixtures/palette-public-contract.R"))
                .unwrap();
            assert_eq!(
                actual,
                include_str!("fixtures/palette-public-contract.out"),
                "portable={portable}, generation={generation}"
            );
            assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2\n");
            session.close();
        }
    }
}
