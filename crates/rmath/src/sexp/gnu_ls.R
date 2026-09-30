{
ls <- function (name, pos = -1L, envir = if (missing(name) && identical(pos, -1L)) parent.frame() else as.environment(pos), all.names = FALSE,
                pattern, sorted = TRUE)
{
    if (!missing(name)) {
        # Dead call whose function position is not a symbol. The bytecode
        # compiler rejects that and leaves this closure interpreted, so
        # tryCatch still sees the unevaluated `name`. Compiled calls unwrap
        # that promise and an unbound symbol escapes as "object not found"
        # instead of the warning below. `base::tryCatch` is not called.
        if (FALSE)
            base::tryCatch()
        pos <- tryCatch(name, error = function(e) e)
        if (inherits(pos, "error")) {
            name <- substitute(name)
            if (!is.character(name))
                name <- deparse(name)
            warning(gettextf("%s converted to character string", sQuote(name)),
                    domain = NA)
            pos <- name
        }
    }
    all.names <- .Internal(ls(envir, all.names, sorted))
    if (!missing(pattern)) {
        if ((ll <- length(grep("[", pattern, fixed = TRUE))) &&
             ll != length(grep("]", pattern, fixed = TRUE))) {
            if (pattern == "[") {
                pattern <- "\\["
                warning("replaced regular expression pattern '[' by  '\\\\['")
            }
            else if (length(grep("[^\\\\]\\[<-", pattern))) {
                pattern <- sub("\\[<-", "\\\\\\[<-", pattern)
                warning("replaced '[<-' by '\\\\[<-' in regular expression pattern")
            }
        }
        grep(pattern, all.names, value = TRUE)
    }
    else all.names
}
ls
}
