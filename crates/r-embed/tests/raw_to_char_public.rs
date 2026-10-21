//! Original raw bytes, multiple mode, native tags and recoverable diagnostics.
use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn raw_to_char_public_cases_and_recovery() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let cases = include_str!("fixtures/raw-to-char-contract.R")
        .lines()
        .filter(|line| line.starts_with("stopifnot("))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 15);
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
    assert!(failures.is_empty(), "rawToChar failures: {failures:?}");
}

#[test]
fn complete_raw_to_char_public_contract() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/raw-to-char-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
}
