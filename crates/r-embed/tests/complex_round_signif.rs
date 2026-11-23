use r_embed::RSession;

#[test]
fn complex_round_and_signif_keep_gnu_recycling_na_and_names() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"
            x <- setNames(c(1.25+2.5i, -3.75+4.5i, NA_complex_), letters[1:3])
            expected <- setNames(c(1.2+2.5i, -4+4i, NA_complex_), letters[1:3])
            identical(round(x, c(1, 0)), expected) &&
                identical(signif(x, c(2, 1)), expected) &&
                identical(round(complex(), 1), complex())
        "#
            )
            .unwrap(),
        "[1] TRUE\n"
    );
}
