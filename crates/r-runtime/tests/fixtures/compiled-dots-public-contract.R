local({
    consume <- function(x, empty, ..., tail=9L) {
        print(missing(empty))
        print(substitute(x))
        print(substitute(list(...)))
        print(sys.call())
        gc()
        print(x)
        print(list(...))
        print(tail)
    }
    forward <- function(x, empty, ...) consume(x=x, empty=empty, ..., tail=7L)
    for (f in list(forward, compiler::cmpfun(forward))) {
        f({cat("forced x\n"); 11L}, tag={cat("forced dot\n"); 13L})
    }
    forwarded <- compiler::cmpfun(function(...) target(..., na.rm=TRUE))
    target <- sum
    print(forwarded(1L, NA, 3L))
    target <- function(..., na.rm) c(sum(...), na.rm)
    print(forwarded(1L, 3L))
    quoted <- compiler::cmpfun(function(...) target(...))
    target <- quote
    print(quoted(stop("must stay lazy")))
    bad <- compiler::cmpfun(function(...) stop(...))
    tryCatch(bad("broken"), error=function(e) {
        print(conditionMessage(e))
        print(conditionCall(e))
    })
    bad_context <- compiler::cmpfun(function() sum(...))
    print(tryCatch(bad_context(), error=conditionMessage))
})
