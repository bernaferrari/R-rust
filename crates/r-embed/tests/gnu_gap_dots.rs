//! GNU R 4.6.1 `...` / DDVAL behavior.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R Under development (unstable) (2026-08-27 r90451)` / 4.6.1).

use r_embed::RSession;

fn raw_expression(bytes: &[u8]) -> String {
    format!(
        "as.raw(c({}))",
        bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(",")
    )
}

fn load(session: &mut RSession, bytes: &[u8]) {
    session
        .eval(&format!("f <- unserialize({})", raw_expression(bytes)))
        .unwrap();
}

fn eval_err(session: &mut RSession, expr: &str) -> String {
    session.eval(expr).expect_err(expr).to_string()
}

#[test]
fn dots_first_element_matches_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(...) ..1; identical(f(10), 10)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn dots_missing_uses_gnu_message() {
    let mut session = RSession::new().unwrap();
    let err = eval_err(&mut session, "f <- function(...) ..1; f()");
    assert!(
        err.contains("the ... list contains fewer than 1 element"),
        "{err}"
    );
}

#[test]
fn dots_invalid_index_uses_gnu_message() {
    let mut session = RSession::new().unwrap();
    let err = eval_err(&mut session, "h <- function(...) ..2; h(1)");
    assert!(
        err.contains("the ... list contains fewer than 2 elements"),
        "{err}"
    );
}

#[test]
fn dots_used_outside_dots_function_uses_gnu_message() {
    let mut session = RSession::new().unwrap();
    let top = eval_err(&mut session, "..1");
    assert!(
        top.contains("..1 used in an incorrect context, no ... to look in"),
        "{top}"
    );
    let inside = eval_err(&mut session, "g <- function() ..1; g()");
    assert!(
        inside.contains("..1 used in an incorrect context, no ... to look in"),
        "{inside}"
    );
}

#[test]
fn missing_ddval_matches_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("m <- function(...) missing(..1); identical(c(m(), m(1)), c(TRUE, FALSE))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_ddval_reads_first_dot() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-dots/ddval1.rds"),
    );
    assert_eq!(session.eval("identical(f(10), 10)").unwrap().trim(), "[1] TRUE");
    let err = eval_err(&mut session, "f()");
    assert!(
        err.contains("the ... list contains fewer than 1 element"),
        "{err}"
    );
}

#[test]
fn imported_gnu_ddval_second_dot_matches_gnu() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-dots/ddval2.rds"),
    );
    assert_eq!(session.eval("identical(f(1, 9), 9)").unwrap().trim(), "[1] TRUE");
    let err = eval_err(&mut session, "f(1)");
    assert!(
        err.contains("the ... list contains fewer than 2 elements"),
        "{err}"
    );
}

#[test]
fn list_dots_forwards_without_ddval() {
    // Contrast: `list(...)` does not need `..1` / DDVAL.
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(...) list(...); identical(f(a=1, 2), list(a=1, 2))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

