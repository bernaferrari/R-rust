use r_embed::RSession;

#[test]
fn portable_iconv_raw_and_unicode_codecs_match_pinned_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/iconv-raw-contract.R"))
            .unwrap(),
        include_str!("fixtures/iconv-raw-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn invalid_utf8_printing_preserves_bytes_and_valid_unicode_columns() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/invalid-utf8-print-contract.R"))
            .unwrap(),
        include_str!("fixtures/invalid-utf8-print-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}
