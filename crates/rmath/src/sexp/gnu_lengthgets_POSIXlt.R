function(x, value) {
    filled <- if (isTRUE(attr(x, "balanced"))) x else balancePOSIXlt(x, fill.only = TRUE)
    r <- lapply(unclass(filled), `length<-`, value)
    class(r) <- oldClass(x)
    attr(r, "tzone") <- attr(x, "tzone")
    attr(r, "balanced") <- if (isTRUE(attr(x, "balanced")) && trunc(value) <= length(x)) TRUE else NA
    r
}
