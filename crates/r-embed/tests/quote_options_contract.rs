use r_embed::RSession;

#[test]
fn quote_options_and_formals_follow_original_base_closures() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/quote-options-contract.R"))
            .unwrap(),
        include_str!("fixtures/quote-options-contract.out")
    );
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}

#[test]
fn unicode_vectors_and_matrices_use_gnu_display_columns() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/unicode-print-width-contract.R"))
            .unwrap(),
        include_str!("fixtures/unicode-print-width-contract.out")
    );
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
