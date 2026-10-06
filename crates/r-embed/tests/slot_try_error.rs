use r_embed::{RSession, RuntimePathPolicy};

/// GNU R 4.6.1 `method-dispatch.R`: `try(letters@foo)` is a try-error whose
/// message is the no-applicable-`@` condition, and evaluation continues.
#[test]
fn try_catches_no_applicable_at_method() {
    let mut session = RSession::new().unwrap();

    let caught = session
        .eval(
            r#"c(
                inherits(try(letters@foo, silent=TRUE), "try-error"),
                grepl("no applicable method for `@`", try(letters@foo, silent=TRUE), fixed=TRUE)
            )"#,
        )
        .unwrap_or_else(|err| panic!("try(letters@foo) escaped the condition system: {err}"));
    assert_eq!(
        caught.trim(),
        "[1] TRUE TRUE",
        "try(letters@foo, silent=TRUE) was not a try-error with the GNU @ message: {caught}"
    );

    let message = session
        .eval(r#"tryCatch(letters@foo, error=function(e) conditionMessage(e))"#)
        .unwrap_or_else(|err| panic!("tryCatch(letters@foo) escaped: {err}"));
    assert!(
        message.contains("no applicable method for `@`"),
        "tryCatch message: {message}"
    );

    let stopped = session
        .eval(r#"inherits(try(stop("boom"), silent=TRUE), "try-error")"#)
        .unwrap_or_else(|err| panic!("try(stop(\"boom\")) escaped: {err}"));
    assert_eq!(
        stopped.trim(),
        "[1] TRUE",
        "stop(\"boom\") inside try(silent=TRUE) was not a try-error: {stopped}"
    );

    let top = session.eval("letters@foo");
    let top_msg = match &top {
        Ok(value) => panic!("top-level letters@foo succeeded: {value}"),
        Err(err) => err.to_string(),
    };
    assert!(
        top_msg.contains("no applicable method for `@`"),
        "top-level letters@foo should stay an error: {top_msg}"
    );
}

#[test]
fn explicit_error_call_survives_try_and_handler_callbacks() {
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        assert_eq!(
            session
                .eval(include_str!(
                    "fixtures/explicit-error-call-public-contract.R"
                ))
                .unwrap(),
            "[1] TRUE\n",
            "portable={portable}"
        );
        assert_eq!(
            session
                .eval(include_str!(
                    "fixtures/try-condition-call-public-contract.R"
                ))
                .unwrap(),
            "[1] TRUE\n",
            "implicit stop attribution portable={portable}"
        );
    }
}
