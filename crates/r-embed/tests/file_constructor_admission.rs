use r_embed::{RSession, RValue, RuntimePathPolicy};

#[test]
fn file_constructor_matches_pinned_gnu_scalar_and_logical_admission() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    session.enable_browser_files();
    let result = session
        .eval_result(include_str!(
            "../../rmath/src/mainutils/connections/file/constructor-contract.R"
        ))
        .unwrap();
    assert_eq!(result.value, RValue::Logical(Some(true)));
}
