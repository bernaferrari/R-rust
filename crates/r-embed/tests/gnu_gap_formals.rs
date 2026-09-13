//! GNU R 4.6.1 `formals<-` / `body<-` on closures.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R version 4.6.1 (2026-06-24)`).
//! Getters already match GNU; the replacement primitives are missing.

use r_embed::RSession;

#[test]
fn formals_and_body_getters_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(x=10) x+1; identical(as.list(formals(f)), list(x=10)) && identical(f(), 11) && identical(paste(deparse(body(f)), collapse=' '), 'x + 1')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn formals_gets_replaces_defaults_and_keeps_body() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(x) x+1; formals(f) <- alist(x=10); identical(f(), 11) && identical(as.list(formals(f)), list(x=10)) && identical(paste(deparse(body(f)), collapse=' '), 'x + 1')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn body_gets_replaces_body_and_keeps_formals() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(x) x+1; body(f) <- quote(x*2); identical(f(3), 6) && identical(names(formals(f)), 'x') && identical(paste(deparse(body(f)), collapse=' '), 'x * 2')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn formals_gets_on_primitive_uses_gnu_error() {
    let mut session = RSession::new().unwrap();
    let err = session
        .eval("formals(sum) <- alist(x=)")
        .expect_err("formals<- on a primitive");
    assert!(
        err.to_string().contains("use of NULL environment is defunct"),
        "{err}"
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
