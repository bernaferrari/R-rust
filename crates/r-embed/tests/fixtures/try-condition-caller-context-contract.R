local({
    protected_call <- quote(doTryCatch(return(expr), name, parentenv, handler))
    caught <- tryCatch(stop("top"), error = identity)
    stopifnot(identical(conditionCall(caught), protected_call))
    attempted <- try(stop("try"), silent = TRUE)
    stopifnot(identical(conditionCall(attr(attempted, "condition")), protected_call),
              identical(as.character(attempted), "Error in try(stop(\"try\"), silent = TRUE) : try\n"))
    caught_warning <- tryCatch(warning("warning"), warning = identity)
    stopifnot(identical(conditionCall(caught_warning), protected_call))
    for (compiled in c(FALSE, TRUE)) {
        nested <- function() stop("nested")
        nested_warning <- function() warning("nested warning")
        protected <- function() {
            seen <- NULL
            caught <- tryCatch(withCallingHandlers(nested(),
                error = function(e) { gc(); seen <<- e }), error = identity)
            stopifnot(identical(seen, caught), identical(conditionCall(caught), quote(nested())),
                      identical(conditionCall(tryCatch(stop("protected"), error = identity)), protected_call))
            invisible(TRUE)
        }
        if (compiled) {
            nested <- compiler::cmpfun(nested)
            nested_warning <- compiler::cmpfun(nested_warning)
            protected <- compiler::cmpfun(protected)
        }
        stopifnot(identical(conditionCall(tryCatch(nested(), error = identity)), quote(nested())),
                  identical(conditionCall(tryCatch(nested_warning(), warning = identity)), quote(nested_warning())),
                  protected(),
                  is.null(conditionCall(tryCatch(stop("no call", call. = FALSE), error = identity))))
    }
    TRUE
})
