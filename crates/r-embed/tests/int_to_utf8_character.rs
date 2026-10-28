use r_embed::RSession;

#[test]
fn int_to_utf8_character_values_use_gnu_integer_coercion() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(
            r#"local({
            warnings <- character()
            value <- withCallingHandlers(
                intToUtf8(c("65", "bad"), multiple = TRUE),
                warning = function(w) {
                    warnings <<- c(warnings, conditionMessage(w))
                    gc()
                    invokeRestart("muffleWarning")
                }
            )
            factor_error <- tryCatch(
                intToUtf8(factor("65")),
                error = function(e) conditionMessage(e)
            )
            environment_error <- tryCatch(
                intToUtf8(new.env()),
                error = function(e) conditionMessage(e)
            )
            identical(value, c("A", NA_character_)) &&
                identical(warnings, "NAs introduced by coercion") &&
                identical(intToUtf8(c(TRUE, FALSE), multiple = TRUE), c("\001", "")) &&
                identical(intToUtf8(list(65L)), "A") &&
                identical(intToUtf8(as.complex(65)), "A") &&
                identical(factor_error, "argument 'x' must be an integer vector") &&
                identical(environment_error, "environments cannot be coerced to other types")
        })"#,
        )
        .unwrap();
    assert_eq!(got, "[1] TRUE\n");
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
