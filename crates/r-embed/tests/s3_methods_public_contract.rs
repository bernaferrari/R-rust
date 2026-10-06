use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn public_s3_reflection_and_summary_match_gnu_under_both_package_policies() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        for (name, source, expected) in [
            (
                "original methods registry",
                include_str!("../../../tests/conformance/cases/284_methods_runtime_registry.R"),
                include_str!("../../../tests/conformance/golden/284_methods_runtime_registry.out"),
            ),
            (
                "public reflection and complete summary workflow",
                include_str!("fixtures/s3-methods-public-contract.R"),
                include_str!("fixtures/s3-methods-public-contract.out"),
            ),
            (
                "original namespace metadata and registered lazy methods",
                include_str!("fixtures/namespace-s3-startup-contract.R"),
                include_str!("fixtures/namespace-s3-startup-contract.out"),
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
