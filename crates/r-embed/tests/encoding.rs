//! GNU Encoding() / Encoding<-.
//!
//! ASCII strings never take a declared encoding. Non-ASCII can be marked
//! UTF-8 / latin1 / bytes.

use r_embed::RSession;

#[test]
fn encoding_tags_and_mixed_text_follow_pinned_gnu_contract() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/encoding-tag-contract.R"))
            .unwrap(),
        include_str!("fixtures/encoding-tag-contract.out")
    );
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}

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

#[test]
fn mixed_text_translation_survives_collection_during_format_and_paste() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval(r#"local({
        u <- rawToChar(as.raw(c(0xc3, 0xa9))); Encoding(u) <- "UTF-8"
        l <- rawToChar(as.raw(0xe9)); Encoding(l) <- "latin1"
        f <- paste0(u, ":%s")
        gctorture(TRUE); on.exit(gctorture(FALSE))
        z <- sprintf(f, c(l,l)); p <- paste(c(u,u), l)
        identical(Encoding(z), c("UTF-8","UTF-8")) &&
          identical(lapply(z, charToRaw), list(as.raw(c(0xc3,0xa9,0x3a,0xc3,0xa9)), as.raw(c(0xc3,0xa9,0x3a,0xc3,0xa9)))) &&
          identical(Encoding(p), c("UTF-8","UTF-8")) &&
          identical(lapply(p, charToRaw), list(as.raw(c(0xc3,0xa9,0x20,0xc3,0xa9)), as.raw(c(0xc3,0xa9,0x20,0xc3,0xa9))))
    })"#).unwrap(), "[1] TRUE\n");
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
