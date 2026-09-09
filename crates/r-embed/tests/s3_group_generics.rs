//! S3 UseMethod / NextMethod / group-generic coverage beyond the grid edit
//! and do.call embed tests. Group generics (Math/Ops/Summary/Complex) must
//! reach user methods via DispatchGroup rather than falling through to the
//! numeric defaults.

use r_embed::RSession;

fn eval_eq(session: &mut RSession, code: &str, expected: &str) {
    let value = session
        .eval(code)
        .unwrap_or_else(|e| panic!("{code} => {e}"));
    assert_eq!(value.trim(), expected, "code was: {code}");
}

#[test]
fn usemethod_and_nextmethod_walk_class_chain() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        f <- function(x) UseMethod("f")
        f.default <- function(x) "d"
        f.c <- function(x) paste("c", NextMethod())
        f.b <- function(x) paste("b", NextMethod())
        f.a <- function(x) paste("a", NextMethod())
        x <- 1; class(x) <- c("a", "b", "c")
        f(x)
        "#,
        "[1] \"a b c d\"",
    );
}

#[test]
fn usemethod_honors_explicit_object_argument() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        g <- function(x, y) UseMethod("g", y)
        g.default <- function(x, y) "def"
        g.bar <- function(x, y) "bar"
        g(1, structure(2, class="bar"))
        "#,
        "[1] \"bar\"",
    );
}

#[test]
fn nextmethod_forwards_dots_and_defaults() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        h <- function(x, ...) UseMethod("h")
        h.default <- function(x, extra = 0) x + extra
        h.a <- function(x, ...) NextMethod()
        result <- h(structure(1, class = "a"), extra = 5)
        paste(as.numeric(result), class(result), sep="|")
        "#,
        "[1] \"6|a\"",
    );
}

#[test]
fn math_group_generic_dispatches_and_preserves_class_via_nextmethod() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        Math.foo <- function(x, ...) structure(NextMethod(), class = class(x))
        x <- structure(c(1, -2, 3), class = "foo")
        r <- abs(x)
        paste(paste(r, collapse = ","), paste(class(r), collapse = ","), sep = "|")
        "#,
        "[1] \"1,2,3|foo\"",
    );
}

#[test]
fn ops_group_generic_dispatches_for_custom_class() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        Ops.foo <- function(e1, e2) structure(NextMethod(), class = "foo")
        x <- structure(1:3, class = "foo")
        r <- x + 1
        paste(paste(r, collapse = ","), paste(class(r), collapse = ","), sep = "|")
        "#,
        "[1] \"2,3,4|foo\"",
    );
}

#[test]
fn summary_group_generic_dispatches() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        called <- FALSE
        Summary.foo <- function(..., na.rm = FALSE) { called <<- TRUE; NextMethod() }
        x <- structure(1:5, class = "foo")
        paste(sum(x), called, sep = "|")
        "#,
        "[1] \"15|TRUE\"",
    );
}

#[test]
fn complex_group_generic_dispatches() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        Complex.foo <- function(z) structure(NextMethod(), class = class(z))
        z <- structure(1 + 2i, class = "foo")
        r <- Re(z)
        paste(r, paste(class(r), collapse = ","), sep = "|")
        "#,
        "[1] \"1|foo\"",
    );
}

#[test]
fn right_hand_and_conflicting_ops_follow_numeric_fallback() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        Ops.foo <- function(e1, e2) structure(NextMethod(), class="foo")
        r <- 1 + structure(2, class="foo")
        paste(as.numeric(r), class(r), sep="|")
    "#,
        "[1] \"3|foo\"",
    );
    eval_eq(
        &mut session,
        r#"
        Ops.left <- function(e1, e2) 10
        Ops.right <- function(e1, e2) 20
        x <- structure(1, class="left")
        y <- structure(2, class="right")
        as.numeric(suppressWarnings(x + y))
    "#,
        "[1] 3",
    );
}

#[test]
fn portable_math_dispatch_and_primitive_aliases_keep_their_identity() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        Math.foo <- function(x, ...) structure(NextMethod(), class="foo")
        g <- log
        z <- g(structure(1, class="foo"))
        plus <- `+`
        paste(as.numeric(z), class(z), plus(1, 2), sep="|")
    "#,
        "[1] \"0|foo|3\"",
    );
}

#[test]
fn arithmetic_preserves_longer_operand_attributes_with_left_precedence() {
    let mut session = RSession::new().unwrap();
    eval_eq(
        &mut session,
        r#"
        a <- structure(c(1,2), label="left", class="foo")
        b <- structure(c(3,4), label="right", note="kept")
        x <- a + b
        y <- a + c(1,2,3,4)
        z <- a + c(1i,2i)
        paste(attr(x,"label"),attr(x,"note"),class(x),class(y),class(z),sep="|")
    "#,
        "[1] \"left|kept|foo|numeric|foo\"",
    );
}
