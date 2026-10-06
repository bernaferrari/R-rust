local({
    helper <- get(".handleSimpleError", baseenv())
    stopifnot(identical(environment(helper), asNamespace("base")),
              identical(names(formals(helper)), c("h", "msg", "call")),
              !exists(".handleSimpleError", .GlobalEnv, inherits = FALSE))
    observed <- NULL
    still_in_function <- FALSE
    fail <- function() stop("calling failure")
    caught <- tryCatch(withCallingHandlers(fail(), error = function(e) {
        observed <<- e
        still_in_function <<- any(vapply(sys.calls(), identical, logical(1), quote(fail())))
        gc()
    }), error = identity)
    stopifnot("stop calling handler" = inherits(observed, "simpleError"), identical(conditionMessage(observed), "calling failure"),
              identical(conditionCall(observed), quote(fail())), identical(caught, observed), still_in_function)

    observed <- NULL
    caught <- tryCatch(withCallingHandlers(stop("without call", call. = FALSE), error = function(e) {
        observed <<- e
        gc()
    }), error = identity)
    stopifnot(is.null(conditionCall(observed)), identical(conditionMessage(observed), "without call"),
              identical(caught, observed))

    seen <- list()
    fail_direct <- function() .Internal(stop(TRUE, "direct failure"))
    caught <- tryCatch(withCallingHandlers(fail_direct(), error = function(e) {
        seen[[length(seen) + 1L]] <<- e
        gc()
    }), error = identity)
    stopifnot(length(seen) == 1L, identical(conditionMessage(seen[[1L]]), "direct failure"),
              identical(conditionCall(seen[[1L]]), conditionCall(caught)), identical(caught, seen[[1L]]))

    observed <- NULL
    caught <- tryCatch(withCallingHandlers(utils::`?`(callingHelpProbe(1)), error = function(e) {
        observed <<- e
        gc()
    }), error = identity)
    stopifnot("help calling handler" = inherits(observed, "simpleError"), identical(caught, observed),
              identical(conditionCall(observed), quote(.helpForCall(topicExpr, parent.frame()))))

    original <- structure(list(message = "custom", call = quote(source_call(1L)), token = 42L),
                          class = c("callingProbe", "error", "condition"))
    observed <- NULL
    caught <- tryCatch(withCallingHandlers(stop(original), callingProbe = function(e) {
        observed <<- e
        gc()
    }), error = identity)
    stopifnot(identical(observed, original), identical(caught, original))
    # Original GNU-compiled public error routes, with caller expressions intact.
    f <- function(p) (p[1] - 2)^2 + (p[2] + 1)^2
    probes <- list(function() dmultinom(c(1, 2), prob = c(0.5)),
                   function() dmultinom(c(1), prob = c(NA)),
                   function() relevel(factor(c("a", "b")), "z"),
                   function() constrOptim(c(2, 0), f, NULL, ui = rbind(c(-1, 0)), ci = -1))
    expected_calls <- list(quote(dmultinom(c(1, 2), prob = c(0.5))),
                           quote(dmultinom(c(1), prob = c(NA))),
                           quote(relevel.factor(factor(c("a", "b")), "z")),
                           quote(constrOptim(c(2, 0), f, NULL, ui = rbind(c(-1, 0)), ci = -1)))
    for (i in seq_along(probes)) {
        observed <- NULL
        caught <- tryCatch(withCallingHandlers(probes[[i]](), error = function(e) {
            observed <<- e; gc()
        }), error = identity)
        stopifnot(identical(observed, caught), identical(conditionCall(observed), expected_calls[[i]]))
    }
    TRUE
})
