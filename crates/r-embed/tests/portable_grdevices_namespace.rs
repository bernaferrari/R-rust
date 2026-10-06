use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn portable_grdevices_exports_and_namespace_identity_survive_session_recreation() {
    for generation in 0..2 {
        let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(
            Vec::new(),
            std::env::temp_dir(),
        ))
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!(
                    "fixtures/grdevices-namespace-public-contract.R"
                ))
                .unwrap(),
            "[1] TRUE\n",
            "generation={generation}"
        );
        assert_eq!(
            session
                .eval("cat(paste(sort(getNamespaceExports('grDevices'),method='radix'),collapse='\\n'))")
                .unwrap(),
            include_str!("fixtures/grdevices-exports.out"),
            "original pinned GNU exports generation={generation}"
        );
        session.close();
    }
}
