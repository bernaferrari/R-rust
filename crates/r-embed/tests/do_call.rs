use r_embed::RSession;

#[test]
fn do_call_resolves_functions_and_expressions_in_explicit_environment() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
        e <- new.env(); e$x <- 7L; e$f <- function(x) x + 1L
        x <- 2L
        a <- do.call("f", list(quote(x)), envir=e)
        b <- do.call(identity, list(quote(x)), quote=TRUE, envir=e)
        c <- do.call(function(x) x + 3L, list(4L), envir=e)
        d <- do.call(args=list(quote(x)), what="f", env=e)
        cat(a, identical(b,quote(x)), c, d)
    "#,
        )
        .unwrap();
    assert_eq!(result.trim(), "8 TRUE 7 8");
}

#[test]
fn do_call_rejects_invalid_inputs_and_survives_collection() {
    let mut session = RSession::new().unwrap();
    let result = session
        .eval(
            r#"
        bad <- function(expr) tryCatch({force(expr); FALSE}, error=function(e) TRUE)
        a <- bad(do.call(1, list()))
        b <- bad(do.call(identity, 1))
        c <- bad(do.call(identity, list(1), envir=1))
        d <- bad(do.call(identity, list(1), quote=NA))
        v <- do.call(function(x,y) {gc(); list(x,y)}, list(list(a=1),list(b=2)))
        e <- bad(do.call(identity,list(1),envir=NULL))
        cat(a,b,c,d,e,identical(v,list(list(a=1),list(b=2))))
    "#,
        )
        .unwrap();
    assert_eq!(result.trim(), "TRUE TRUE TRUE TRUE TRUE TRUE");
}
