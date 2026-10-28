use r_embed::RSession;

#[test]
fn int_to_utf8_flags_reject_missing_values_and_coerce_logical_scalars() {
    let mut session = RSession::new().unwrap();
    let got = session
        .eval(r#"local({
            multiple_error <- tryCatch(
                intToUtf8(65L, multiple = NA),
                error = function(e) conditionMessage(e)
            )
            surrogate_error <- tryCatch(
                intToUtf8(c(0xD83D, 0xDE00), allow_surrogate_pairs = NA),
                error = function(e) conditionMessage(e)
            )
            empty_multiple_error <- tryCatch(
                intToUtf8(c(65L, 66L), multiple = logical()),
                error = function(e) conditionMessage(e)
            )
            empty_surrogate_error <- tryCatch(
                intToUtf8(65L, allow_surrogate_pairs = logical()),
                error = function(e) conditionMessage(e)
            )
            null_multiple_error <- tryCatch(
                intToUtf8(c(65L, 66L), multiple = NULL),
                error = function(e) conditionMessage(e)
            )
            null_surrogate_error <- tryCatch(
                intToUtf8(c(0xD83D, 0xDE00), allow_surrogate_pairs = NULL),
                error = function(e) conditionMessage(e)
            )
            positional_null_error <- tryCatch(
                intToUtf8(x = c(65L, 66L), NULL),
                error = function(e) conditionMessage(e)
            )
            reordered_null_error <- tryCatch(
                intToUtf8(NULL, x = c(65L, 66L)),
                error = function(e) conditionMessage(e)
            )
            named_before_positional_error <- tryCatch(
                intToUtf8(NULL, allow_surrogate_pairs = NULL, x = c(65L, 66L)),
                error = function(e) conditionMessage(e)
            )
            unknown_name_error <- tryCatch(
                intToUtf8(65L, unknown = TRUE),
                error = function(e) conditionMessage(e)
            )
            duplicate_name_error <- tryCatch(
                intToUtf8(65L, multiple = TRUE, multiple = FALSE),
                error = function(e) conditionMessage(e)
            )
            extra_positional_error <- tryCatch(
                intToUtf8(65L, FALSE, FALSE, TRUE),
                error = function(e) conditionMessage(e)
            )
            identical(multiple_error, "argument 'multiple' must be TRUE or FALSE") &&
                identical(surrogate_error, "argument 'allow_surrogate_pairs' must be TRUE or FALSE") &&
                identical(empty_multiple_error, "argument 'multiple' must be TRUE or FALSE") &&
                identical(empty_surrogate_error, "argument 'allow_surrogate_pairs' must be TRUE or FALSE") &&
                identical(null_multiple_error, "argument 'multiple' must be TRUE or FALSE") &&
                identical(null_surrogate_error, "argument 'allow_surrogate_pairs' must be TRUE or FALSE") &&
                identical(positional_null_error, "argument 'multiple' must be TRUE or FALSE") &&
                identical(reordered_null_error, "argument 'multiple' must be TRUE or FALSE") &&
                identical(named_before_positional_error, "argument 'multiple' must be TRUE or FALSE") &&
                identical(unknown_name_error, "unused argument (unknown = TRUE)") &&
                identical(duplicate_name_error, "formal argument \"multiple\" matched by multiple actual arguments") &&
                identical(extra_positional_error, "unused argument (TRUE)") &&
                identical(intToUtf8(c(65L, 66L)), "AB") &&
                identical(intToUtf8(c(65L, 66L), multiple = 1L), c("A", "B")) &&
                identical(intToUtf8(TRUE, x = c(65L, 66L)), c("A", "B")) &&
                identical(intToUtf8(c(65L, 66L), multip = TRUE), c("A", "B")) &&
                identical(intToUtf8(c(0xD83D, 0xDE00), allow = TRUE), "😀") &&
                identical(intToUtf8(c(65L, 66L), multiple = c(TRUE, FALSE)), c("A", "B")) &&
                identical(intToUtf8(c(65L, 66L), multiple = "TRUE"), c("A", "B")) &&
                identical(intToUtf8(c(65L, 66L), multiple = 0L), "AB") &&
                identical(intToUtf8(c(0xD83D, 0xDE00), allow_surrogate_pairs = TRUE), "😀")
        })"#)
        .unwrap();
    assert_eq!(got, "[1] TRUE\n");
    assert_eq!(session.eval("1L + 1L").unwrap(), "[1] 2\n");
}
