use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn warning_and_message_suppression_preserves_dynamic_handler_order() {
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
                    "fixtures/calling-warning-handler-public-contract.R"
                ))
                .unwrap(),
            include_str!("fixtures/calling-warning-handler-public-contract.out"),
            "portable={portable}"
        );
        assert_eq!(
            session
                .eval(include_str!(
                    "../../../tests/conformance/cases/288_with_calling_handlers.R"
                ))
                .unwrap(),
            include_str!("../../../tests/conformance/golden/288_with_calling_handlers.out"),
            "portable={portable}: original complete calling-handler workflow"
        );
    }
}
