use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn rep_len_admission_contract_matches_gnu_under_both_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!("fixtures/rep-len-admission-contract.R"))
                .unwrap(),
            include_str!("fixtures/rep-len-admission-contract.out"),
            "portable={portable}"
        );
        assert_eq!(
            session
                .eval(include_str!(
                    "../../../tests/conformance/cases/260_length_replacement.R"
                ))
                .unwrap(),
            include_str!("../../../tests/conformance/golden/260_length_replacement.out"),
            "portable={portable}: original complete length-replacement workflow"
        );
        session.close();
    }
}
