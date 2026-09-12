//! GNU R 4.6.1 MAKECLOSURE / nested `function()` compilation.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R version 4.6.1 (2026-06-24)`).
//! GNU `compiler::cmpfun` emits MAKECLOSURE for a nested function and
//! the compiled closure returns 3. rport's portable compiler rejects the
//! body. Imported uncompressed XDR v2 fixtures already execute.

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

#[test]
fn interpreted_nested_function_matches_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("g <- function() { f <- function(x) x+1; f(2) }; identical(g(), 3)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn cmpfun_nested_function_matches_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("g <- compiler::cmpfun(function() { f <- function(x) x+1; f(2) }); identical(g(), 3)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_makeclosure_nested_call_matches_gnu() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-makeclosure/nested.rds"),
    );
    assert_eq!(
        session.eval("identical(f(), 3)").unwrap().trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_makeclosure_returned_closure_matches_gnu() {
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-makeclosure/nested-return.rds"),
    );
    assert_eq!(
        session
            .eval("g <- f(); identical(g(4), 5)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn cmpfun_local_fun_calls_keep_arguments_lazy() {
    // GNU MAKEPROM semantics: a closure call never evaluates arguments
    // the callee does not force.
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "f <- compiler::cmpfun(function() { g <- function(y) 5; g(stop('boom')) }); identical(f(), 5)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("h <- compiler::cmpfun(function() { g <- function(y) y; g(1+1) }); identical(h(), 2)")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval(
                "n <- 0; k <- compiler::cmpfun(function() { g <- function(y) { n <<- n + 1; y }; c(g(9), g(8)) }); identical(k(), c(9, 8)) && identical(n, 2)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
