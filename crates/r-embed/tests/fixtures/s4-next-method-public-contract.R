local({
    setClass("NextMatProbe", representation(Dim = "integer", "VIRTUAL"))
    setMethod("[", signature(x = "NextMatProbe", i = "missing", j = "missing", drop = "ANY"),
              function(x, i, j, drop) x)
    removeMethod("[", signature(x = "NextMatProbe", i = "missing", j = "missing", drop = "ANY"))
    setClass("NextBaseProbe", representation(a = "numeric"))
    setClass("NextChildProbe", contains = "NextBaseProbe")
    for (compiled in c(FALSE, TRUE)) {
        parent_method <- function(x, i, j, ..., drop = TRUE) {
            call <- sys.call()
            expression <- substitute(drop)
            absent <- missing(drop)
            gc()
            list(drop = drop, call = call, expression = expression, missing = absent)
        }
        child_method <- function(x, i, j, ..., drop = TRUE) {
            gc()
            callNextMethod()
        }
        if (compiled) {
            parent_method <- compiler::cmpfun(parent_method)
            child_method <- compiler::cmpfun(child_method)
        }
        setMethod("[", "NextBaseProbe", parent_method)
        setMethod("[", "NextChildProbe", child_method)
        x <- new("NextBaseProbe")
        y <- new("NextChildProbe")
        direct <- x[1, drop = FALSE]
        inherited <- y[1, drop = FALSE]
        stopifnot("direct original call" = identical(direct$call, quote(x[1, drop = FALSE])),
                  identical(direct$drop, FALSE), identical(direct$expression, FALSE), !direct$missing,
                  "inherited drop" = identical(inherited$drop, FALSE), !inherited$missing,
                  identical(inherited$call, quote(.nextMethod(x = x, i = i, drop = drop))),
                  identical(inherited$expression, quote(drop)))
        inherited_default <- y[1]
        stopifnot(identical(inherited_default$drop, TRUE), inherited_default$missing)
    }
    stopifnot(identical(asNamespace("methods"), environment(methods::callNextMethod)))
    TRUE
})
