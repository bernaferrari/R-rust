//! Differential serialize/unserialize round-trips for core SEXP types.
//! Builds on the save/load + compiled-closure coverage: focuses on
//! `identical()` fidelity after binary `serialize`/`unserialize`, including
//! pairlists that previously failed because `allocList` used a null CDR.

use r_embed::RSession;

fn eval_true(session: &mut RSession, code: &str) {
    let value = session
        .eval(code)
        .unwrap_or_else(|e| panic!("{code} => {e}"));
    assert!(
        value.trim() == "[1] TRUE" || value.trim() == "TRUE",
        "expected TRUE from `{code}`, got {value:?}"
    );
}

#[test]
fn serialize_roundtrips_core_types_with_identical() {
    let mut session = RSession::new().unwrap();
    eval_true(
        &mut session,
        r#"
        check <- function(x) identical(unserialize(serialize(x, NULL)), x)
        all(c(
          check(1:5),
          check(c(a=1L,b=2L)),
          check(c(1+2i, NA)),
          check(as.raw(0:3)),
          check(list(a=1L, b=list(c=TRUE))),
          check(pairlist(a=1L, b="x")),
          check(pairlist(1L, 2L)),
          check(as.pairlist(list(one=1L, two="y"))),
          check(quote(foo(1L, bar=2.5))),
          {
            f <- function(x, y=2L) x + y
            g <- unserialize(serialize(f, NULL))
            identical(formals(f), formals(g)) && identical(g(3L), 5L)
          },
          check(expression(1L, 1 + 2)),
          check({
            x <- 1:3
            attr(x, "label") <- "score"
            attr(x, "meta") <- list(v=2L)
            x
          }),
          check(factor(c("b","a",NA,"b"), levels=c("a","b"))),
          check(data.frame(x=1:2, y=c("a","b"), stringsAsFactors=FALSE))
        ))
        "#,
    );
}

#[test]
fn serialize_pairlist_bytes_stable_across_roundtrip() {
    let mut session = RSession::new().unwrap();
    eval_true(
        &mut session,
        r#"
        pl <- pairlist(a=1L, b="x", c=TRUE)
        pl2 <- unserialize(serialize(pl, NULL))
        identical(pl, pl2) && identical(as.list(pl), as.list(pl2)) &&
          identical(names(pl), names(pl2)) &&
          identical(serialize(pl, NULL), serialize(pl2, NULL))
        "#,
    );
}

#[test]
fn save_load_ascii_pairlist_matches_serialize_roundtrip() {
    let mut session = RSession::new().unwrap();
    eval_true(
        &mut session,
        r#"
        f <- tempfile()
        pl <- pairlist(alpha=1L, beta="x", gamma=TRUE)
        save(pl, file=f, ascii=TRUE)
        pl_ser <- unserialize(serialize(pl, NULL))
        rm(pl)
        load(f)
        identical(pl, pl_ser) && identical(names(pl), c("alpha","beta","gamma")) &&
          identical(as.list(pl), as.list(pl_ser))
        "#,
    );
}
