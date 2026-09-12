//! GNU R 4.6.1 `match.call` with `...` after DDVAL.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R version 4.6.1 (2026-06-24)`).
//! The call structure is present (`as.list(mc)` has a `...` pairlist),
//! but language `names()` / `[[` by tag still miss GNU.

use r_embed::RSession;

#[test]
fn match_call_expand_dots_true_forwards_named_and_positional() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(a, ...) match.call(); g <- function(...) f(1, ...); identical(g(x=2, 3), quote(f(a=1, x=2, 3)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_expand_dots_false_keeps_dots_pairlist() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(a, ...) match.call(expand.dots=FALSE); mc <- f(1, x=2, 3); identical(as.list(as.list(mc)[['...']]), list(x=2, 3))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn match_call_language_names_and_tag_extract_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "f <- function(a, ...) match.call(expand.dots=FALSE);                  mc <- f(1, x=2, 3);                  identical(names(mc), c('', 'a', '...')) &&                  identical(as.list(mc[['...']]), list(x=2, 3)) &&                  identical(names(quote(f(a=1, b=2))), c('', 'a', 'b')) &&                  identical(quote(f(a=1, b=2))[['a']], 1)"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
