use r_embed::{RSession, RValue};

#[test]
fn source_comments_end_at_cr_lf_and_crlf_without_changing_string_hashes() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval_result(
            r##"
            source_with_separator <- function(separator) {
              path <- tempfile(fileext=".R")
              text <- paste0("# comment", separator,
                             "answer <- 2+3", separator, "answer")
              writeBin(charToRaw(text), path)
              target <- new.env(parent=baseenv())
              echoed <- capture.output(source(path, local=target, echo=TRUE))
              parsed <- parse(file=path, keep.source=TRUE)
              start_lines <- vapply(attr(parsed, "srcref"),
                                    function(ref) as.integer(ref)[[1L]], integer(1))
              source_lines <- attr(parsed, "srcfile")$lines
              unlink(path)
              identical(target$answer, 5) &&
                any(grepl("> answer <- 2 + 3", echoed, fixed=TRUE)) &&
                identical(start_lines, c(2L, 3L)) &&
                identical(source_lines, c("# comment", "answer <- 2+3", "answer"))
            }
            literal_path <- tempfile(fileext=".R")
            writeBin(charToRaw("literal <- \"# inside string\"\rvalue <- literal"),
                     literal_path)
            literal_env <- new.env(parent=baseenv())
            source(literal_path, local=literal_env)
            unlink(literal_path)
            all(vapply(c("\n", "\r\n", "\r"), source_with_separator,
                       logical(1))) &&
              identical(literal_env$value, "# inside string")
            "##,
        )
        .expect("evaluate source CR comment contract");
    assert_eq!(result.value, RValue::Logical(Some(true)));
}
