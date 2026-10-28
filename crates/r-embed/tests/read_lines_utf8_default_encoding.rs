use r_embed::RSession;

#[test]
fn read_lines_preserves_default_native_mark_for_valid_utf8() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(
            r#"local({
                path <- tempfile()
                writeBin(as.raw(c(0x0a, 0xc3, 0xa9, 0x0a)), path)

                gctorture(TRUE)
                on.exit(gctorture(FALSE))
                by_path <- readLines(path)
                gctorture(FALSE)
                gc()
                by_path_utf8 <- readLines(path, encoding = "UTF-8")
                con <- file(path, open = "rt")
                by_connection <- readLines(con)
                close(con)

                con <- file(path, open = "rt")
                explicitly_utf8 <- readLines(con, encoding = "UTF-8")
                close(con)
                unlink(path)
                empty_path <- tempfile()
                writeBin(raw(), empty_path)
                empty <- readLines(empty_path)
                unlink(empty_path)

                stopifnot(identical(Encoding(by_path), c("unknown", "unknown")))
                stopifnot(identical(Encoding(by_path_utf8), c("unknown", "UTF-8")))
                stopifnot(identical(Encoding(by_connection), c("unknown", "unknown")))
                stopifnot(identical(Encoding(explicitly_utf8), c("unknown", "UTF-8")))
                stopifnot(identical(charToRaw(by_path[2L]), as.raw(c(0xc3, 0xa9))))
                stopifnot(identical(charToRaw(by_path_utf8[2L]), as.raw(c(0xc3, 0xa9))))
                stopifnot(identical(charToRaw(explicitly_utf8[2L]), as.raw(c(0xc3, 0xa9))))
                stopifnot(length(empty) == 0L, !anyNA(by_path), !anyNA(by_connection))
                TRUE
            })"#,
        )
        .unwrap();
    assert_eq!(got, "[1] TRUE\n");
}
