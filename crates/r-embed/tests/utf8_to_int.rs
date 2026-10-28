use r_embed::RSession;

#[test]
fn utf8_to_int_ignores_extra_strings_with_gnu_warning() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(r#"local({
            warnings <- character()
            value <- withCallingHandlers(
                utf8ToInt(c("é", "ignored")),
                warning = function(w) {
                    warnings <<- c(warnings, conditionMessage(w))
                    gc()
                    invokeRestart("muffleWarning")
                }
            )
            identical(value, as.integer(233)) &&
                identical(warnings, "argument should be a character vector of length 1\nall but the first element will be ignored")
        })"#)
        .unwrap();
    assert_eq!(got, "[1] TRUE\n");
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
