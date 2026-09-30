use r_embed::RSession;

/// GNU R 4.6.1 `tests/reg-tests-1e.R` PR#18336. A 6x6 sparse count table
/// must fail inside FEXACT with a hash key past `INT_MAX`, rather than
/// returning an `htest` from the first four cells.
#[test]
fn fisher_test_too_full_table_reports_hash_key_overflow() {
    let mut session = RSession::new().unwrap();
    let err = session
        .eval(
            "d <- matrix(c(1,0,5,2,1,90, 2,1,0,2,3,89, 0,0,0,1,0,14, 0,0,0,0,0,5, 0,0,0,0,0,2, 0,0,0,0,0,2), nrow=6, byrow=TRUE); fisher.test(d)",
        )
        .expect_err("6x6 fisher.test must signal the FEXACT hash-key error");
    let message = err.to_string();
    assert!(
        message.contains("hash key") && message.contains("> INT_MAX"),
        "{message}"
    );
}

/// GNU R 4.6.1 `tests/reg-tests-1e.R` PR#18367. Empty `[[` and `[[<-`
/// are `MissingSubscriptError`, and the condition keeps the call.
#[test]
fn missing_subscript_keeps_the_call_for_extract_and_assign() {
    let mut session = RSession::new().unwrap();
    let out = session
        .eval(
            r#"
            local({
              E <- tryCatch(c(a = 1, 2)[[]], error = function(e) e)
              xx <- c(a = 1, 2:3)
              E2 <- tryCatch(xx[[]], error = function(e) e)
              EN <- tryCatch(NULL[[]], error = function(e) e)
              EA <- tryCatch(xx[[]] <- pi, error = function(e) e)
              ok <- inherits(E, "MissingSubscriptError") &&
                identical(E$call, quote(c(a = 1, 2)[[]])) &&
                identical(conditionMessage(E), "missing subscript") &&
                identical(class(E), class(E2)) &&
                identical(class(E), class(EN)) &&
                identical(conditionMessage(E2), "missing subscript") &&
                identical(conditionMessage(EN), "missing subscript") &&
                all(c("call", "object") %in% names(EN)) &&
                identical(EN$call, quote(NULL[[]])) &&
                is.null(EN$object) &&
                inherits(EA, "MissingSubscriptError") &&
                identical(EA$call, quote(xx[[]] <- pi)) &&
                identical(conditionMessage(EA), "missing subscript")
              z <- c(1, 2)
              ED <- tryCatch(xx[[]] <- `[[<-`(z, , pi), error = function(e) e)
              ok <- ok && inherits(ED, "MissingSubscriptError") &&
                identical(ED$call, quote(`[[<-`(z, , pi))) &&
                identical(conditionMessage(ED), "missing subscript")
              if (ok) "ok" else paste0("call=", deparse1(EA$call), " direct=", deparse1(ED$call), " class=", paste(class(EA), collapse = "|"))
            })
            "#,
        )
        .unwrap();
    assert_eq!(out.trim(), "[1] \"ok\"");
}
