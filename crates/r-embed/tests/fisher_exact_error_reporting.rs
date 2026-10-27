use r_embed::RSession;
use std::process::Command;

const FEXACT_HASH_OVERFLOW: &str = r#"
reli <- cbind(Si = c(2121, 100, 27, 0),
              av = c(4700, 216, 67, 0),
              Nc = c(6234, 2461, 502, 14))
"#;

#[test]
fn fexact_hash_key_error_is_catchable_and_uncaught_propagates() {
    let mut session = RSession::new().unwrap();
    let output = session
        .eval(&format!(
            r#"
            {FEXACT_HASH_OVERFLOW}
            tried <- try(fisher.test(reli, workspace=2000000))
            caught <- tryCatch(
              fisher.test(reli, workspace=2000000),
              error=function(e) list(class=class(e)[1], message=conditionMessage(e),
                                     call=deparse1(conditionCall(e))))
            wrapper <- function() fisher.test(reli, workspace=2000000)
            nested <- tryCatch(
              wrapper(),
              error=function(e) list(class=class(e)[1], message=conditionMessage(e),
                                     call=deparse1(conditionCall(e))))
            stopifnot(inherits(tried, "try-error"),
                      grepl("Error in fisher.test(reli, workspace = 2e+06)",
                            as.character(tried)[1], fixed=TRUE),
                      identical(deparse1(attr(tried, "condition")$call),
                                "fisher.test(reli, workspace = 2e+06)"),
                      identical(caught$class, "simpleError"),
                      grepl("FEXACT error 501", caught$message, fixed=TRUE),
                      grepl("hash table key", caught$message, fixed=TRUE))
            stopifnot(identical(caught$call, "fisher.test(reli, workspace = 2e+06)"),
                      identical(nested$class, caught$class),
                      identical(nested$message, caught$message),
                      identical(nested$call, caught$call))
            cat("fexact-error-caught\n")
            "#
        ))
        .unwrap();
    assert!(output.contains("fexact-error-caught"), "{output}");

    let mut uncaught = RSession::new().unwrap();
    let error = uncaught
        .eval(&format!(
            "{FEXACT_HASH_OVERFLOW}\nfisher.test(reli, workspace=2000000)"
        ))
        .expect_err("an uncaught FEXACT error must propagate to the embedding caller");
    assert!(
        error.to_string().contains("FEXACT error 501")
            && error.to_string().contains("hash table key"),
        "{error}"
    );
}

#[test]
fn fexact_error_emits_no_raw_stderr() {
    const PROBE: &str = "RPORT_FEXACT_STDERR_PROBE";
    if std::env::var_os(PROBE).is_some() {
        let mut session = RSession::new().unwrap();
        let output = session
            .eval(&format!(
                "{FEXACT_HASH_OVERFLOW}\ntry(fisher.test(reli, workspace=2000000))\ncat(\"caught\\n\")"
            ))
            .unwrap();
        assert!(output.contains("caught"), "{output}");
        return;
    }

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fexact_error_emits_no_raw_stderr", "--nocapture"])
        .env(PROBE, "1")
        .output()
        .expect("spawn isolated FEXACT error probe");
    assert!(
        output.status.success(),
        "child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("FEXACT error 501"),
        "the caught R error must not also write raw FEXACT text to stderr: {stderr}"
    );
}
