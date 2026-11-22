use r_embed::RSession;

#[test]
fn native_table_readers_handle_growth_and_quoted_eof() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"count <- function(text, sep='', skip=0L, blskip=TRUE) {
  con <- rawConnection(charToRaw(text))
  on.exit(close(con))
  .External('C_countfields', con, sep, '"', skip, blskip, '#', PACKAGE='utils')
}
head_lines <- function(bytes, skip=FALSE) {
  con <- rawConnection(bytes)
  on.exit(close(con))
  .External('C_readtablehead', con, 5L, '#', TRUE, '"', ',', skip, PACKAGE='utils')
}
long <- strrep('x', 2500L)
counts <- count(paste0(strrep('a,b\n', 1000L), 'c,d'), ',')
error <- tryCatch(count('skip\na b\n\n"bad\nfragment', skip=1L),
                  error=conditionMessage)
warning_text <- ''
with_nul <- withCallingHandlers(
  head_lines(as.raw(c(97,0,98,10,99,10))),
  warning=function(w) {
    warning_text <<- conditionMessage(w)
    invokeRestart('muffleWarning')
  })
stopifnot(
  identical(counts, rep.int(2L, 1001L)),
  identical(error, 'quoted string on line 2 terminated by EOF'),
  identical(count('a,b\n"bad\nfragment', ','), c(2L, NA_integer_, 1L)),
  identical(head_lines(charToRaw(paste0(long, '\n# comment\n\n"b\nc",d\n'))),
            c(long, '"b\nc",d')),
  identical(with_nul, c('a', 'c')),
  identical(warning_text, 'line 1 appears to contain embedded nulls'),
  identical(head_lines(as.raw(c(97,0,98,10,99,10)), TRUE), c('ab', 'c')))
cat('table-scanner-checked: PASS\n')"#
            )
            .unwrap(),
        "table-scanner-checked: PASS\n"
    );
}
