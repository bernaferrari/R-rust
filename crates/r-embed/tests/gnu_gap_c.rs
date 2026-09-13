//! GNU R 4.6.1 `c()` edge cases still visible without STARTC/DFLTC.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R Under development (unstable) (2026-08-27 r90451)` / 4.6.1).
//! GNU 4.6.1 compiler no longer emits STARTC/DFLTC; `c(...)` emits DODOTS.

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
fn interpreted_c_empty_and_null_are_null() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("is.null(c()) && is.null(c(NULL))").unwrap().trim(), "[1] TRUE");
}

#[test]
fn interpreted_c_drops_null_and_keeps_later_values() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(c(NULL, 1), 1) && typeof(c(NULL, 1)) == 'double' && is.null(names(c(NULL, 1)))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(c(a=1, b=NULL), c(a=1)) && identical(names(c(a=1, b=NULL)), 'a')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("identical(c(a=NULL, b=1), c(b=1)) && identical(names(c(a=NULL, b=1)), 'b')")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn interpreted_c_pairlist_keeps_pairlist_tags_as_names() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(c(pairlist(a=1L), 2L), list(a=1L, 2L)) && identical(names(c(pairlist(a=1L), 2L)), c('a', ''))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn interpreted_c_pairlist_only_keeps_both_tags() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(c(pairlist(a=1L, b=2L)), list(a=1L, b=2L)) && typeof(c(pairlist(a=1L, b=2L))) == 'list'")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn interpreted_c_list_plus_pairlist_keeps_pairlist_tag() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(c(list(a=1), pairlist(b=2)), list(a=1, b=2)) && identical(names(c(list(a=1), pairlist(b=2))), c('a', 'b'))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn imported_gnu_c_dots_matches_gnu_values() {
    // BASEGUARD / GETFUN c / DODOTS / CALL / RETURN.
    let mut session = RSession::new().unwrap();
    load(
        &mut session,
        include_bytes!("fixtures/gnu-gap-c/c-dots.rds"),
    );
    assert_eq!(
        session
            .eval("identical(f(1L, 2L), c(1L, 2L)) && identical(f(a=1, 2), c(a=1, 2))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
