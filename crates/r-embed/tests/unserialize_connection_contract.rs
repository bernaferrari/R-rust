use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn unserialize_connection_contract_matches_gnu_under_both_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/unserialize-connection-contract.R"))
                .unwrap(),
            include_str!("fixtures/unserialize-connection-contract.out"),
            "portable={portable}"
        );
        session.close();
    }
}
