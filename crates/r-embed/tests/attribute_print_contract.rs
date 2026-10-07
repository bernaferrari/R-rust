use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn public_attribute_printing_preserves_output_and_live_edges_under_both_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/attribute-print-public-contract.R"))
                .unwrap_or_else(|error| panic!("portable={portable}; {error}")),
            include_str!("fixtures/attribute-print-public-contract.out"),
            "portable={portable}"
        );
        assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
    }
}
