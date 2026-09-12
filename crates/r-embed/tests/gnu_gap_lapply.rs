//! GNU R 4.6.1 `lapply` / `vapply` names.
//!
//! Oracle: Homebrew `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`
//! (`R version 4.6.1 (2026-06-24)`).
//! `vapply` already copies atomic names; `lapply` / `sapply` drop them.

use r_embed::RSession;

#[test]
fn lapply_named_atomic_keeps_gnu_names() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(names(lapply(c(a=1, b=2), identity)), c('a', 'b')) && identical(unname(lapply(c(a=1, b=2), identity)), list(1, 2))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn sapply_named_atomic_keeps_gnu_names() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("identical(names(sapply(c(a=1, b=2), identity)), c('a', 'b')) && identical(unname(sapply(c(a=1, b=2), identity)), c(1, 2))")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn lapply_list_and_null_names_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("is.null(names(lapply(1:2, identity))) && identical(names(lapply(list(a=1, 2), identity)), c('a', '')) && identical(lapply(NULL, identity), list())")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn vapply_names_and_dropping_match_gnu() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "v <- vapply(1:2, function(i) c(x=i, y=i+1), numeric(2));                  identical(names(vapply(c(a=1, b=2), identity, numeric(1))), c('a', 'b')) &&                  is.null(names(vapply(c(a=1, b=2), identity, numeric(1), USE.NAMES=FALSE))) &&                  identical(names(vapply(c('a', 'b'), nchar, integer(1))), c('a', 'b')) &&                  identical(dim(v), c(2L, 2L)) && identical(dimnames(v)[[1]], c('x', 'y')) &&                  is.null(dimnames(v)[[2]])"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}
