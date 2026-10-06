use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn base_print_methods_match_gnu_under_both_package_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        for (name, source, expected) in [
            (
                "original primitive-alias controls",
                include_str!(
                    "../../../tests/conformance/cases/388_no_print_summary_method_aliases.R"
                ),
                include_str!(
                    "../../../tests/conformance/golden/388_no_print_summary_method_aliases.out"
                ),
            ),
            (
                "base method identity, source, dispatch and compiled workflows",
                include_str!("fixtures/base-print-methods-public-contract.R"),
                include_str!("fixtures/base-print-methods-public-contract.out"),
            ),
            (
                "compiled lazy dots calls, tags, missingness and conditions",
                include_str!("fixtures/compiled-dots-public-contract.R"),
                include_str!("fixtures/compiled-dots-public-contract.out"),
            ),
        ] {
            assert_eq!(
                session
                    .eval(source)
                    .unwrap_or_else(|error| panic!("{name}; portable={portable}; {error}")),
                expected,
                "{name}; portable={portable}"
            );
            assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
        }
    }
}
