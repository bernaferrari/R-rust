Vectorize <- function(FUN, vectorize.args = arg.names, SIMPLIFY = TRUE,
                      USE.NAMES = TRUE)
{
    arg.names <- as.list(formals(FUN))
    arg.names[["..."]] <- NULL
    arg.names <- names(arg.names)
    vectorize.args <- as.character(vectorize.args)
    if (!length(vectorize.args)) return(FUN)
    if (!all(vectorize.args %in% arg.names))
        stop("must specify names of formal arguments for 'vectorize'")
    collisions <- arg.names %in% c("FUN", "SIMPLIFY", "USE.NAMES", "vectorize.args")
    if (any(collisions))
        stop(sQuote("FUN"), " may not have argument(s) named ",
             paste(sQuote(arg.names[collisions]), collapse = ", "))
    rm(arg.names, collisions)
    (function() {
        FUNV <- function() {
            args <- lapply(as.list(match.call())[-1L], eval, parent.frame())
            names <- names(args) %||% character(length(args))
            dovec <- names %in% vectorize.args
            do.call("mapply", c(FUN = FUN,
                                args[dovec],
                                MoreArgs = list(args[!dovec]),
                                SIMPLIFY = SIMPLIFY,
                                USE.NAMES = USE.NAMES))
        }
        formals(FUNV) <- formals(FUN)
        environment(FUNV) <- parent.env(environment())
        FUNV
    })()
}
