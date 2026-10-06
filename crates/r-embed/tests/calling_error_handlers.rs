use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn calling_error_handlers_receive_original_conditions_before_unwind() {
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
                    "fixtures/calling-error-handler-public-contract.R"
                ))
                .unwrap(),
            "[1] TRUE\n",
            "portable={portable}"
        );
    }
}
