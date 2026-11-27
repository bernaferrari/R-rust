local({
    old <- options(useFancyQuotes = FALSE)
    on.exit(options(old))
    caught <- try(letters@foo, silent = TRUE)
    stopifnot(identical(conditionCall(attr(caught, "condition")), quote(letters@foo)),
              identical(as.character(caught),
                        "Error in letters@foo : \n  no applicable method for `@` applied to an object of class \"character\"\n"))
    direct <- tryCatch(letters@foo, error = identity)
    stopifnot(identical(conditionCall(direct), quote(letters@foo)))
    observed <- NULL
    called <- tryCatch(withCallingHandlers(letters@foo, error = function(e) {
        observed <<- e
        gc()
    }), error = identity)
    stopifnot(identical(conditionCall(observed), quote(letters@foo)),
              identical(conditionCall(called), quote(letters@foo)))
    replacement <- tryCatch(withCallingHandlers(letters@foo, error = function(e) {
        gc()
        stop("handler")
    }), error = identity)
    stopifnot(identical(conditionMessage(replacement), "handler"),
              identical(conditionCall(replacement), quote(h(simpleError(msg, call)))))
    TRUE
})
