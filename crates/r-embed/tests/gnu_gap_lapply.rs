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

#[test]
fn vapply_uses_checked_cells_for_scalar_and_column_results() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                "raw_ok <- identical(vapply(1:2, function(i) as.raw(c(i, i+1L)), raw(2)), matrix(as.raw(c(1,2,2,3)), 2)); complex_ok <- identical(vapply(1:2, function(i) c(i+1i, i+2i), complex(2)), matrix(c(1+1i,1+2i,2+1i,2+2i), 2)); logical_ok <- identical(vapply(1:2, function(i) c(TRUE, NA), logical(2)), matrix(rep(c(TRUE, NA), 2), 2)); scalar_ok <- identical(vapply(1:2, function(i) i+0i, complex(1)), c(1+0i,2+0i)); empty_ok <- identical(vapply(1:2,function(i) integer(0),integer(0)), matrix(integer(),0,2)); raw_ok && complex_ok && logical_ok && scalar_ok && empty_ok"
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn vapply_keeps_first_result_names_through_gc_and_coercion() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval(
                r#"v_native <- function(X,FUN,FUN.VALUE,USE.NAMES=TRUE,...) .Internal(vapply(X,FUN,FUN.VALUE,USE.NAMES))
f <- function(i) { invisible(gc()); c(y=i>1L,z=i==1L) }
x <- c(a=1L,b=2L)
want <- matrix(c(0,1,1,0),2L,dimnames=list(c('y','z'),c('a','b')))
stopifnot(identical(vapply(x,f,numeric(2)),want), identical(v_native(x,f,numeric(2)),want))
cat('apply-roots-ok\n')"#
            )
            .unwrap(),
        "apply-roots-ok\n"
    );
}
