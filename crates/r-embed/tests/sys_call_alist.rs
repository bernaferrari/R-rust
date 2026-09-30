//! `sys.call()` forced as a promise argument must name the promise's home
//! frame. Oracle: GNU R 4.6.1 `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript`.
//!
//! `names(f(x=, y=2))` is `c("x", "y")` when
//! `f <- function(...) as.list(sys.call())[-1L]`.

use r_embed::RSession;

#[test]
fn sys_call_inside_as_list_argument_sees_caller() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("f <- function(...) as.list(sys.call())[-1L]; names(f(x=, y=2))")
            .unwrap()
            .trim(),
        "[1] \"x\" \"y\""
    );
}

#[test]
fn direct_sys_call_is_the_callee_not_an_internal_frame() {
    let mut session = RSession::new().unwrap();
    assert_eq!(
        session
            .eval("g <- function(a) sys.call(); deparse(g(1))")
            .unwrap()
            .trim(),
        "[1] \"g(1)\""
    );
}

#[test]
fn parent_frame_from_nested_function_sees_caller() {
    let mut session = RSession::new().unwrap();
    // `$` may be invisible; compare the value. GNU prints 123 for the
    // same nested `parent.frame()$marker`.
    assert_eq!(
        session
            .eval(
                "outer <- function() { marker <- 123L; inner <- function() parent.frame()$marker; identical(inner(), 123L) }; outer()",
            )
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
}

#[test]
fn base_alist_keeps_missing_tags_as_symbols() {
    let mut session = RSession::new().unwrap();
    // GNU R 4.6.1 dput: `list(x = , y = 2)` and `list(a = 1, b = )`.
    // The missing tag is a symbol whose character form is empty.
    assert_eq!(
        session.eval("names(alist(x=, y=2))").unwrap().trim(),
        "[1] \"x\" \"y\""
    );
    assert_eq!(
        session
            .eval("capture.output(dput(alist(x=, y=2)))")
            .unwrap()
            .trim(),
        "[1] \"list(x = , y = 2)\""
    );
    assert_eq!(
        session
            .eval("capture.output(dput(alist(a=1, b=)))")
            .unwrap()
            .trim(),
        "[1] \"list(a = 1, b = )\""
    );
    assert_eq!(
        session
            .eval("a <- alist(x=, y=2); typeof(a[[1]])")
            .unwrap()
            .trim(),
        "[1] \"symbol\""
    );
    assert_eq!(
        session
            .eval("is.symbol(alist(x=, y=2)[[1]])")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("as.character(alist(x=, y=2)[[1]])")
            .unwrap()
            .trim(),
        "[1] \"\""
    );
    assert_eq!(
        session
            .eval("b <- alist(a=1, b=); typeof(b[[2]])")
            .unwrap()
            .trim(),
        "[1] \"symbol\""
    );
    assert_eq!(
        session
            .eval("is.symbol(alist(a=1, b=)[[2]])")
            .unwrap()
            .trim(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("as.character(alist(a=1, b=)[[2]])")
            .unwrap()
            .trim(),
        "[1] \"\""
    );
    assert_eq!(
        session.eval("alist(a=1, b=)[[1]]").unwrap().trim(),
        "[1] 1"
    );
    assert_eq!(
        session.eval("deparse(body(alist))").unwrap().trim(),
        "[1] \"as.list(sys.call())[-1L]\""
    );
}
