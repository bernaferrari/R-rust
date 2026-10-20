use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn internal_dput_preserves_arity_connections_visibility_and_public_closure() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/internal-dput-contract.R"))
            .unwrap(),
        include_str!("fixtures/internal-dput-contract.out")
    );
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
