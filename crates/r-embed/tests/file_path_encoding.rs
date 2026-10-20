use r_embed::RSession;

#[test]
fn file_paths_preserve_pinned_gnu_encodings_and_components() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/file-path-encoding-contract.R"))
            .unwrap(),
        include_str!("fixtures/file-path-encoding-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}
