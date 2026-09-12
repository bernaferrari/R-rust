//! GNU nchar type=chars counts Unicode code points, not bytes.
//! type=width uses East-Asian display width (中 = 2).
//!
//! Oracle: Homebrew / pinned GNU 4.6.1 in a UTF-8 session. The conformance
//! harness runs LC_ALL=C, where GNU treats CE_NATIVE as one-byte and this
//! case cannot live there.

use r_embed::RSession;

#[test]
fn nchar_chars_counts_code_points_not_bytes() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(r#"paste(nchar(c("a", "é", "中", "café")), collapse=",")"#)
            .unwrap()
            .trim(),
        r#"[1] "1,1,1,4""#
    );
}

#[test]
fn nchar_bytes_width_and_na_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"
x <- c("a", "é", "中", "café")
identical(paste(nchar(x, type="bytes"), collapse=","), "1,2,3,5") &&
  identical(paste(nchar(x, type="chars"), collapse=","), "1,1,1,4") &&
  identical(paste(nchar(x, type="width"), collapse=","), "1,1,2,4") &&
  is.na(nchar(NA_character_)) &&
  identical(nchar("café", type="c"), 4L)
"#
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
