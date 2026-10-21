//! try strings and geterrmessage preserve the canonical diagnostic bytes.
use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn try_byte_diagnostic_public_cases_and_recovery() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let cases = include_str!("fixtures/try-byte-diagnostic-contract.R")
        .lines()
        .filter(|line| line.starts_with("stopifnot("))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 12);
    let mut failures = Vec::new();
    for (index, code) in cases.iter().enumerate() {
        let actual = session.eval(&format!("{code}\nTRUE"));
        if !matches!(actual.as_deref(), Ok("[1] TRUE\n")) {
            failures.push((index + 1, actual));
        }
        assert_eq!(
            session.eval("1+1").unwrap(),
            "[1] 2\n",
            "recovery after case {}",
            index + 1
        );
    }
    assert!(
        failures.is_empty(),
        "tryByteDiagnostic failures: {failures:?}"
    );
}

#[test]
fn complete_try_byte_diagnostic_public_contract() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/try-byte-diagnostic-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
}

#[test]
fn gnu_try_construction_formals_visibility_and_output_contract() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/try-construction-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
    assert_eq!(session.eval("1L+1L").unwrap(), "[1] 2\n");
}

#[test]
fn upstream_staging_uses_the_runtime_compatible_try_closure() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(session.eval(r#"local({e<-environment(getSrcLines);z<-eval(quote(try(stop("staged"),silent=TRUE)),e);identical(as.character(z),"Error in try(stop(\"staged\"), silent = TRUE) : staged\n")})"#).unwrap(), "[1] TRUE\n");
}
