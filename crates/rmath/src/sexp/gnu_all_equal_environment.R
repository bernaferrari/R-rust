function(target, current, all.names = TRUE, evaluate = TRUE, ...)
{
    if (!is.environment(target)) return("'target' is not an environment")
    if (!is.environment(current)) return("'current' is not an environment")
    if (identical(target, current))
        return(TRUE)

    ## GNU dynGet("__all.eq.E__"): walk the call stack for a seen-pairs env.
    ae.run <- NULL
    n <- sys.nframe()
    if (n >= 1L) {
        for (i in n:1) {
            e <- sys.frame(i)
            if (exists("__all.eq.E__", envir = e, inherits = FALSE)) {
                ae.run <- get("__all.eq.E__", envir = e, inherits = FALSE)
                break
            }
        }
    }
    if (is.null(ae.run)) {
        "__all.eq.E__" <- environment()
    } else {
        do1 <- function(em) {
            if (identical(target, em$target) && identical(current, em$current))
                TRUE
            else if (!is.null(em$mm))
                do1(em$mm)
            else {
                e <- new.env(parent = emptyenv())
                e$target <- target
                e$current <- current
                em$mm <- e
                FALSE
            }
        }
        if (do1(ae.run)) return(TRUE)
    }

    if (evaluate) {
        Lt <- as.list(target, all.names = all.names)
        Lc <- as.list(current, all.names = all.names)
        if (identical(Lt, Lc))
            TRUE
        else all.equal.list(Lt, Lc, ...)
    } else {
        nt <- sort(names(target))
        nc <- sort(names(current))
        if (!identical(nt, nc))
            paste("names of environments differ:", all.equal(nt, nc, ...), collapse = " ")
        else
            "environments contain objects of the same names, but are not identical"
    }
}
