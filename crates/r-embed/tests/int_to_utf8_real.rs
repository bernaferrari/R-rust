use r_embed::RSession;

#[test]
fn int_to_utf8_real_matches_integer_coercion_boundaries_and_fraction_truncation() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(
            r#"local({
            warnings <- character()
            value <- withCallingHandlers(
                intToUtf8(c(NA_real_, NaN, Inf, -Inf, 2147483647, 2147483647.1,
                    -2147483648, -2147483648.1, 2147483648, -2147483649, 65.9),
                    multiple = TRUE),
                warning = function(w) {
                    warnings <<- c(warnings, conditionMessage(w))
                    gc()
                    invokeRestart("muffleWarning")
                }
            )
            identical(value, c(rep(NA_character_, 10), "A")) &&
                identical(warnings, "NAs introduced by coercion to integer range")
        })"#,
        )
        .unwrap();
    assert_eq!(got, "[1] TRUE\n");
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
