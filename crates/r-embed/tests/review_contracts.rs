use r_embed::RSession;

#[test]
fn numerical_layout_coercion_and_tolerance() {
    let mut s = RSession::new().unwrap();
    for code in [
        "stopifnot(isTRUE(all.equal(solve(matrix(c(1,2,3,4),2), c(5,6)), c(-1,2))))",
        "stopifnot(isTRUE(all.equal(solve(matrix(c(1L,2L,3L,4L),2), c(5L,6L)), c(-1,2))))",
        "a <- matrix(c(1,2,3,4),2); stopifnot(isTRUE(all.equal(a %*% solve(a), diag(2))))",
        "a <- matrix(c(1+1i,2+0i,3+0i,4+1i),2); b <- matrix(c(5+1i,6+0i,2+2i,3+1i),2); stopifnot(isTRUE(all.equal(a %*% solve(a,b), b)))",
        "stopifnot(isTRUE(all.equal(solve(a=matrix(c(1,2,3,4),2),c(5,6)),c(-1,2))))",
        "stopifnot(inherits(try(solve(diag(2),NULL),silent=TRUE),'try-error'))",
        "z <- matrix(c(1+1i,2+0i,3+0i,4+1i),2); stopifnot(isTRUE(all.equal(crossprod(z),t(z) %*% z)))",
        "stopifnot(inherits(try(solve(diag(c(1,1e-20))), silent=TRUE), 'try-error'))",
        "stopifnot(isTRUE(all.equal(solve(diag(c(1,1e-20)), tol=0), diag(c(1,1e20)))))",
    ] {
        s.eval(code).unwrap_or_else(|e| panic!("{code}: {e}"));
    }
}

#[test]
fn functional_type_dispatch_laziness_and_visibility() {
    let mut s = RSession::new().unwrap();
    for code in [
        "stopifnot(identical(as.character(quote(c(b,d))),c('c','b','d')))",
        "stopifnot(identical(as.character(quote(f(1+2, x))),c('f','1 + 2','x')))",
        "stopifnot(identical(as.list(quote(a)),list(quote(a))))",
        "stopifnot(identical(names(as.list(function(x=1,y=2)x+y)),c('x','y','')))",
        "stopifnot(identical(as.list(c('Hello ','World','!')),list('Hello ','World','!')))",
        "stopifnot(identical(as.list(as.raw(c(1,2))),list(as.raw(1),as.raw(2))))",
        "stopifnot(identical(as.list(c(1+2i,3+4i)),list(1+2i,3+4i)))",
        "stopifnot(identical(mapply(function(x) x, 1:3), 1:3))",
        "stopifnot(identical(mapply(function(x) x, c('a','b'), USE.NAMES=FALSE), c('a','b')))",
        "stopifnot(is.null(names(mapply(function(x) x, c(a=1,b=2), USE.NAMES=FALSE))))",
        "stopifnot(identical(Filter(function(x) 1, 1:3), 1:3))",
        "stopifnot(identical(class(Filter(function(x) TRUE, factor(c('a','b')))), 'factor'))",
        "stopifnot(identical(vapply(1:2, function(x,y)x, integer(1), y=stop('unused')), 1:2))",
        "stopifnot(withVisible(withCallingHandlers(1))$visible)",
        "stopifnot(identical((1:3)[1,drop=NA], 1L))",
        "stopifnot(!is.null(attr(parse(text='x <- 1',keep.source=TRUE),'srcref')))",
    ] {
        s.eval(code).unwrap_or_else(|e| panic!("{code}: {e}"));
    }
}

#[test]
fn rendering_obeys_function_dispatch_and_preserves_user_bindings() {
    let mut s = RSession::new().unwrap();
    s.eval("counter <- 0L; old <- 11; newd <- 22; result <- 33; plot <- function(...) counter <<- counter + 1L").unwrap();
    let bytes = s
        .render_with_dimensions("plot(x=1:3, y=1:3)", 100, 100)
        .unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    assert_eq!(s.eval("counter").unwrap(), "[1] 1");
    s.eval("stopifnot(old == 11, newd == 22, result == 33)")
        .unwrap();
}
