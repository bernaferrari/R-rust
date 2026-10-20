use r_embed::RSession;

#[test]
fn iconv_nul_diagnostics_match_gnu_bound_and_survive_recovery() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/iconv-nul-diagnostic-contract.R"))
            .unwrap(),
        include_str!("fixtures/iconv-nul-diagnostic-contract.out")
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}
