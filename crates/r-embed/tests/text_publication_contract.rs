use r_embed::RSession;

#[test]
fn text_publication_preserves_gnu_partial_lines_roots_and_capture_destinations() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/text-publication-contract.R"))
            .unwrap(),
        include_str!("fixtures/text-publication-contract.out")
    );
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
