use r_embed::RSession;

#[test]
fn enc2utf8_marks_valid_non_ascii_unknown_bytes_as_utf8() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(
            r#"local({
                value <- c(unknown = "é", ascii = "A", tagged = "ß", missing = NA_character_)
                Encoding(value[1L]) <- "unknown"
                Encoding(value[2L]) <- "unknown"
                gctorture(TRUE)
                on.exit(gctorture(FALSE))
                converted <- enc2utf8(value)
                gctorture(FALSE)
                gc()
                stopifnot(identical(Encoding(converted[1L]), "UTF-8"))
                stopifnot(identical(Encoding(converted[2L]), "unknown"))
                stopifnot(identical(Encoding(converted[3L]), "UTF-8"))
                stopifnot(is.na(converted[4L]))
                stopifnot(identical(charToRaw(converted[1L]), as.raw(c(0xc3, 0xa9))))
                stopifnot(identical(names(converted), names(value)))
                TRUE
            })"#,
        )
        .unwrap();
    assert!(got.ends_with("[1] TRUE\n"), "{got}");
}
