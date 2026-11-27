events <- character()
record <- function(label) function(e) events <<- c(events, label)
clear <- function() { globalCallingHandlers(NULL); events <<- character() }
case <- function(name, expr) { cat(name, "\n"); eval(substitute(expr), parent.frame()); cat("PASS\n") }
clear()
globalCallingHandlers(error = record("global-error"))
case("try-string", {
    result <- withVisible(try(stop("caught"), silent = TRUE))
    stopifnot(inherits(result$value, "try-error"), !result$visible, identical(events, character()))
})
case("tryCatch-string", {
    events <- character()
    result <- tryCatch(stop("caught"), error = identity)
    stopifnot(inherits(result, "error"), identical(conditionMessage(result), "caught"), identical(events, character()))
})
case("local-calling-before-exiting", {
    events <- character()
    result <- try(withCallingHandlers(stop("caught"), error = record("local-error")), silent = TRUE)
    stopifnot(inherits(result, "try-error"), identical(events, "local-error"))
})
case("handler-failure-goes-outward", {
    events <- character()
    result <- tryCatch(tryCatch(stop("inner"), error = function(e) {
        events <<- c(events, "inner-handler"); stop("outer")
    }), error = function(e) { events <<- c(events, "outer-handler"); conditionMessage(e) })
    stopifnot(identical(result, "outer"), identical(events, c("inner-handler", "outer-handler")))
})
case("finally-failure-goes-outward", {
    events <- character()
    result <- tryCatch(tryCatch({events <<- c(events, "body"); 1L}, error = identity,
        finally = {events <<- c(events, "finally"); stop("final")}), error = function(e) {
        events <<- c(events, "outer-handler"); conditionMessage(e)
    })
    stopifnot(identical(result, "final"), identical(events, c("body", "finally", "outer-handler")))
})
case("stack-restored", {
    events <- character()
    signalCondition(simpleError("uncaught"))
    stopifnot(identical(events, "global-error"))
})
clear()
globalCallingHandlers(warning = record("global-warning"), message = record("global-message"), foo = record("global-foo"))
case("warning-exiting", {
    events <- character()
    result <- tryCatch(withCallingHandlers(warning("caught"), warning = record("local-warning")), warning = identity)
    stopifnot(inherits(result, "warning"), identical(events, "local-warning"))
})
case("message-exiting", {
    events <- character()
    result <- tryCatch(withCallingHandlers(message("caught"), message = record("local-message")), message = identity)
    stopifnot(inherits(result, "message"), identical(events, "local-message"))
})
case("custom-exiting", {
    events <- character()
    condition <- structure(list(message = "custom", call = quote(source_call())), class = c("foo", "condition"))
    result <- tryCatch(signalCondition(condition), foo = identity)
    stopifnot(identical(result, condition), identical(events, character()))
})
clear()

case("signal-visible-after-calling", {
    result <- withVisible(withCallingHandlers(signalCondition(simpleCondition("visible")), condition = function(e) invisible(NULL)))
    stopifnot(is.null(result$value), isTRUE(result$visible))
})
case("signal-visible-after-global", {
    globalCallingHandlers(condition = function(e) invisible(NULL))
    result <- withVisible(signalCondition(simpleCondition("visible")))
    stopifnot(is.null(result$value), isTRUE(result$visible))
    clear()
})
case("try-success-invisible", {
    result <- withVisible(try(invisible(42L), silent = TRUE))
    stopifnot(identical(result$value, 42L), !result$visible)
})
case("finally-preserves-invisible-body", {
    result <- withVisible(tryCatch(invisible(1L), finally = 2L))
    stopifnot(identical(result$value, 1L), !result$visible)
})
case("finally-preserves-visible-body", {
    result <- withVisible(tryCatch(1L, finally = invisible(NULL)))
    stopifnot(identical(result$value, 1L), isTRUE(result$visible))
})
case("finally-preserves-invisible-handler", {
    result <- withVisible(tryCatch(stop("x"), error = function(e) invisible(2L), finally = 3L))
    stopifnot(identical(result$value, 2L), !result$visible)
})
case("error-buffer-on-string-exit", {
    .Internal(seterrmessage("seed"))
    result <- tryCatch(stop("x"), error = function(e) geterrmessage())
    stopifnot(identical(result, "x"))
})
case("error-buffer-preserved-for-condition", {
    .Internal(seterrmessage("seed"))
    result <- tryCatch(stop(errorCondition("custom")), error = function(e) geterrmessage())
    stopifnot(identical(result, "seed"))
    result <- tryCatch(signalCondition(simpleError("signal")), error = function(e) geterrmessage())
    stopifnot(identical(result, "seed"))
})
case("stack-order-before-class-order", {
    clear()
    globalCallingHandlers(condition = record("generic"), foo = record("foo"))
    signalCondition(structure(list(message = "x", call = NULL), class = c("foo", "condition")))
    stopifnot(identical(events, c("generic", "foo")))
    clear()
})
case("calling-handler-reentry", {
    events <- character()
    condition <- structure(list(message = "original", call = NULL), class = c("foo", "condition"))
    withCallingHandlers(withCallingHandlers(signalCondition(condition), foo = function(e) {
        events <<- c(events, "inner")
        signalCondition(simpleCondition("nested"))
    }), condition = function(e) events <<- c(events, paste0("outer:", conditionMessage(e))))
    stopifnot(identical(events, c("inner", "outer:nested", "outer:original")))
})
case("compiled-exiting-handler", {
    clear()
    globalCallingHandlers(error = record("global"))
    f <- compiler::cmpfun(function() tryCatch(stop("compiled"), error = function(e) {
        events <<- c(events, "compiled-handler"); conditionMessage(e)
    }))
    stopifnot(identical(f(), "compiled"), identical(events, "compiled-handler"))
    clear()
})
case("collecting-handler-and-finally", {
    condition <- errorCondition("owned", call = quote(original_call()), class = "custom", payload = 1:1000)
    finalizations <- 0L
    result <- tryCatch(stop(condition), error = function(e) {
        condition <<- NULL
        gc(full = TRUE)
        e
    }, finally = {finalizations <<- finalizations + 1L; gc(full = TRUE); invisible(NULL)})
    stopifnot(inherits(result, "custom"), identical(conditionMessage(result), "owned"),
              identical(conditionCall(result), quote(original_call())), identical(result$payload, 1:1000),
              identical(finalizations, 1L))
})
clear()
case("typed-variable", {
    e <- tryCatch(missing_symbol, error=identity)
    stopifnot(identical(class(e), c("objectNotFoundError", "error", "condition")),
              identical(e$name, quote(missing_symbol)), identical(e$mode, "any"))
})
case("typed-formal-promise", {
    meta <- function(name) {
        e <- tryCatch(name, error=identity)
        stopifnot(identical(class(e), c("objectNotFoundError", "error", "condition")),
                  identical(e$name, quote(missing_symbol)), identical(e$mode, "any"))
        paste(deparse(substitute(name)), class(e)[1L], sep="|")
    }
    stopifnot(identical(meta(missing_symbol), "missing_symbol|objectNotFoundError"))
})
case("typed-compiled-formal", {
    meta <- compiler::cmpfun(meta)
    stopifnot(identical(meta(missing_symbol), "missing_symbol|objectNotFoundError"))
})
case("typed-compiled-direct", {
    compiled_direct <- compiler::cmpfun(function() missing_direct)
    e <- tryCatch(compiled_direct(), error=identity)
    stopifnot(identical(class(e), c("objectNotFoundError", "error", "condition")),
              identical(e$name, quote(missing_direct)), identical(e$mode, "any"),
              identical(conditionCall(e), quote(compiled_direct())))
})
case("typed-function", {
    e <- tryCatch(missing_function(), error=identity)
    stopifnot(identical(class(e), c("functionNotFoundError", "objectNotFoundError", "error", "condition")),
              identical(e$name, quote(missing_function)), identical(e$mode, "function"))
})
case("typed-get-mode", {
    e <- tryCatch(get("missing_object", mode="function"), error=identity)
    stopifnot(identical(class(e), c("objectNotFoundError", "error", "condition")),
              identical(e$name, quote(missing_object)), identical(e$mode, "function"))
})
case("typed-exiting-priority", {
    seen <- tryCatch(missing_symbol, objectNotFoundError=function(e) {
        gc(full=TRUE)
        c("specific", as.character(e$name), e$mode)
    }, error=function(e) "broad")
    stopifnot(identical(seen, c("specific", "missing_symbol", "any")))
})
case("typed-calling-once", {
    events <- character()
    e <- tryCatch(withCallingHandlers(missing_symbol, objectNotFoundError=function(e) {
        gc(full=TRUE)
        events <<- c(events, paste(class(e)[1L], as.character(e$name), sep=":"))
    }, error=function(e) events <<- c(events, "broad")), error=identity)
    stopifnot(identical(events, c("objectNotFoundError:missing_symbol", "broad")),
              inherits(e, "objectNotFoundError"), identical(e$mode, "any"))
})
case("typed-handler-error-outward", {
    e <- tryCatch(tryCatch(missing_symbol, objectNotFoundError=function(e) missing_function()),
                  functionNotFoundError=identity)
    stopifnot(inherits(e, "functionNotFoundError"), identical(e$name, quote(missing_function)),
              identical(e$mode, "function"))
})
case("typed-error-buffer", {
    e <- tryCatch(stop("typed-buffer-seed"), error=identity)
    before <- geterrmessage()
    e <- tryCatch(missing_symbol, error=identity)
    stopifnot(identical(geterrmessage(), before), inherits(e, "objectNotFoundError"))
})
case("typed-collection-and-finally", {
    cleaned <- 0L
    e <- tryCatch(get("missing_object"), error=function(e) {
        gc(full=TRUE)
        e
    }, finally={cleaned <<- cleaned+1L; gc(full=TRUE)})
    stopifnot(identical(cleaned, 1L), inherits(e, "objectNotFoundError"),
              identical(e$name, quote(missing_object)), identical(e$mode, "any"))
})
clear()
