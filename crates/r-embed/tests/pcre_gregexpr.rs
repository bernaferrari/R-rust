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
