# R-level compiler package definitions. The pure world evaluates closures
# directly rather than through bytecode, so compilation is the identity
# transformation — GNU's cmpfun contract (a function behaving exactly as
# its input) holds without a compilation step.

cmpfun <- function(f, options = NULL) {
    if (!is.function(f))
        stop("first argument must be a function")
    f
}

compile <- function(e, env = parent.frame(), options = NULL) {
    cmpfun(e)
}

cmpprim <- function(f, options = NULL) {
    f
}
