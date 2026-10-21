//! GNU simple casing, declared encodings and recoverable errors.
use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn independent_portable_case_contracts_recover_in_one_session() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let fixture = include_str!("fixtures/public-case-conversion-contract.R");
    let cases = fixture
        .lines()
        .filter(|line| line.starts_with("stopifnot("))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 21);
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
    assert!(failures.is_empty(), "case failures: {failures:?}");
}

#[test]
fn native_case_profile() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    let code = if std::env::var("LC_ALL").unwrap_or_default() == "C" {
        r#"{x<-rawToChar(as.raw(c(97,233))); y<-toupper(x); identical(charToRaw(y),as.raw(c(65,233))) && identical(Encoding(y),"unknown")}"#
    } else {
        r#"{x<-rawToChar(as.raw(c(97,233))); z<-rawToChar(as.raw(c(195,169))); Encoding(z)<-"unknown"; y<-toupper(z); identical(tryCatch(toupper(x),error=function(e)conditionMessage(e)),"invalid multibyte string 1") && identical(charToRaw(y),as.raw(c(195,137))) && identical(Encoding(y),"unknown")}"#
    };
    assert_eq!(session.eval(code).unwrap(), "[1] TRUE\n");
}

#[test]
fn complete_portable_case_contract() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "tmp")).unwrap();
    assert_eq!(
        session
            .eval(include_str!("fixtures/public-case-conversion-contract.R"))
            .unwrap(),
        "[1] TRUE\n"
    );
}
