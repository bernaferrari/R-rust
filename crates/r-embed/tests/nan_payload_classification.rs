use r_embed::RSession;

#[test]
fn is_nan_distinguishes_quiet_na_payloads_in_real_and_complex_vectors() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/nan-payload-classification.R"))
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
