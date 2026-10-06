use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn vector_print_limits_match_gnu_under_both_package_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!(
                    "fixtures/vector-print-limits-public-contract.R"
                ))
                .unwrap_or_else(|error| panic!("portable={portable}; {error}")),
            include_str!("fixtures/vector-print-limits-public-contract.out"),
            "portable={portable}"
        );
        assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
        assert_eq!(
            session
                .eval(include_str!("fixtures/print-digits-public-contract.R"))
                .unwrap_or_else(|error| panic!("digits; portable={portable}; {error}")),
            include_str!("fixtures/print-digits-public-contract.out"),
            "digits; portable={portable}"
        );
    }
}
