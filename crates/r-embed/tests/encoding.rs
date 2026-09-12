//! GNU Encoding() / Encoding<-.
//!
//! ASCII strings never take a declared encoding. Non-ASCII can be marked
//! UTF-8 / latin1 / bytes.

use r_embed::RSession;

#[test]
fn encoding_ascii_stays_unknown() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"
x <- "a"
Encoding(x) <- "UTF-8"
identical(Encoding(x), "unknown") &&
  { Encoding(x) <- "bytes"; identical(Encoding(x), "unknown") }
"#
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn encoding_set_on_non_ascii() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"
e <- "é"
Encoding(e) <- "bytes"
identical(Encoding(e), "bytes") &&
  { Encoding(e) <- "latin1"; identical(Encoding(e), "latin1") } &&
  { Encoding(e) <- "UTF-8"; identical(Encoding(e), "UTF-8") } &&
  { Encoding(e) <- "unknown"; identical(Encoding(e), "unknown") }
"#
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
