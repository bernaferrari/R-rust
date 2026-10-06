use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn stats_and_graphics_original_imports_survive_fresh_public_sessions() {
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
                    .eval(include_str!("fixtures/stats-namespace-public-contract.R"))
                    .unwrap(),
                include_str!("fixtures/stats-namespace-public-contract.out"),
                "portable={portable}, generation={generation}"
            );
            assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2\n");
            session.close();
        }
    }
}
