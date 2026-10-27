use r_embed::{RSession, RValue};

#[test]
fn read_lines_accepts_cr_lf_crlf_and_keeps_memory_connection_cursors() {
    let path = std::env::temp_dir().join(format!("rport-readlines-cr-{}.bin", std::process::id()));
    let quoted = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let empty_path =
        std::env::temp_dir().join(format!("rport-readlines-empty-{}.bin", std::process::id()));
    let empty_quoted = empty_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut session = RSession::new().expect("session");
    let result = session
        .eval_result(&format!(
            r#"
            path <- "{quoted}"
            writeBin(charToRaw("α\r\rβ"), path)
            path_lines <- readLines(path, warn=FALSE)

            empty_path <- "{empty_quoted}"
            writeBin(raw(), empty_path)
            empty_file <- readLines(empty_path, warn=FALSE)

            boundary_path <- paste0(path, ".boundary")
            boundary_text <- paste0(strrep("a", 8191L), "\r\nβ")
            writeBin(charToRaw(boundary_text), boundary_path)
            boundary_con <- file(boundary_path, open="rt")
            boundary_lines <- readLines(boundary_con, warn=FALSE)
            close(boundary_con)

            file_con <- file(path, open="rt")
            file_first <- readLines(file_con, n=2L, warn=FALSE)
            file_rest <- readLines(file_con, warn=FALSE)
            close(file_con)

            text_con <- textConnection("α\r\rβ")
            text_first <- readLines(text_con, n=2L, warn=FALSE)
            text_rest <- readLines(text_con, warn=FALSE)
            close(text_con)

            tail_con <- textConnection("tail")
            tail_first <- readLines(tail_con, n=1L, warn=FALSE)
            tail_rest <- readLines(tail_con, warn=FALSE)
            close(tail_con)

            raw_con <- rawConnection(charToRaw("α\r\rβ"))
            raw_first <- readLines(raw_con, n=2L, warn=FALSE)
            raw_rest <- readLines(raw_con, warn=FALSE)
            close(raw_con)

            nul_con <- rawConnection(as.raw(c(97L, 0L, 98L, 13L, 99L)))
            nul_default <- readLines(nul_con, warn=FALSE)
            close(nul_con)
            nul_con <- rawConnection(as.raw(c(97L, 0L, 98L, 13L, 99L)))
            nul_skipped <- readLines(nul_con, warn=FALSE, skipNul=TRUE)
            close(nul_con)

            terminators <- list(
              readLines(textConnection("a\nb"), warn=FALSE),
              readLines(textConnection("a\r\nb"), warn=FALSE),
              readLines(textConnection("a\rb"), warn=FALSE),
              readLines(textConnection("a\r"), warn=FALSE),
              readLines(textConnection(""), warn=FALSE)
            )
            unlink(c(path, empty_path, boundary_path))
            identical(path_lines, c("α", "", "β")) &&
              identical(empty_file, character()) &&
              identical(boundary_lines, c(strrep("a", 8191L), "β")) &&
              identical(nul_default, c("a", "c")) &&
              identical(nul_skipped, c("ab", "c")) &&
              identical(file_first, c("α", "")) && identical(file_rest, "β") &&
              identical(text_first, c("α", "")) && identical(text_rest, "β") &&
              identical(tail_first, "tail") && identical(tail_rest, character()) &&
              identical(raw_first, c("α", "")) && identical(raw_rest, "β") &&
              inherits(try(readLines(textConnection("a\r"), n=2L,
                                     ok=FALSE, warn=FALSE), silent=TRUE),
                       "try-error") &&
              identical(terminators, list(c("a", "b"), c("a", "b"),
                                          c("a", "b"), "a", ""))
            "#
        ))
        .expect("evaluate readLines CR separator contract");
    assert_eq!(result.value, RValue::Logical(Some(true)));
}
