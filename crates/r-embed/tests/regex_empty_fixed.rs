use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn empty_fixed_replacement_patterns_error_without_breaking_empty_regex_or_grep() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/regex-empty-fixed-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
}
