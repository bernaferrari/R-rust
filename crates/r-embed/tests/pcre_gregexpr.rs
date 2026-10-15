//! GNU `grepl(..., perl=TRUE)` treats NA in `x` as a non-match.

use r_embed::RSession;

#[test]
fn grepl_perl_na_in_x_is_false() {
    let mut session = RSession::new().expect("session");
    let got = session
        .eval(
            r#"cat(paste(capture.output(dput(grepl("a+", c("baaa", "xyz", NA), perl = TRUE))), collapse = "\n"))"#,
        )
        .unwrap_or_else(|err| panic!("grepl eval failed: {err}"));
    assert_eq!(got.trim(), "c(TRUE, FALSE, FALSE)");
}

#[test]
fn regex_unicode_offsets_context_and_invalid_patterns_match_gnu() {
    let mut session =
        RSession::new_with_path_policy(r_embed::RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/regex-unicode-public-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("1+1").unwrap(), "[1] 2\n");
}
