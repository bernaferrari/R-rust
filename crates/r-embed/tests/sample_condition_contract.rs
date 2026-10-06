use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn sample_conditions_preserve_calls_and_rng_state_under_both_policies() {
    for portable in [false, true] {
        let mut s = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            s.eval(include_str!("fixtures/sample-condition-contract.R"))
                .unwrap(),
            include_str!("fixtures/sample-condition-contract.out"),
            "portable={portable}"
        );
        s.close();
    }
}
