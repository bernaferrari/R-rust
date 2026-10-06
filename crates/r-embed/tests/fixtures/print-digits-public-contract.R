local({
    old <- options(digits = 7, width = 80, scipen = 0, max.print = 9999)
    on.exit(options(old))
    for (x in list(c(0.09999999, 0.2, pi),
                   setNames(c(pi, exp(1)), c("pi", "e")),
                   matrix(c(pi, exp(1), 0.000123456, 123456), 2),
                   c(pi + exp(1)*1i, 0.000123456 + 123456i))) {
        for (d in c(1L, 2L, 6L, 10L)) {
            cat("class:", class(x), "digits:", d, "\n")
            v <- withVisible(print(x, digits = d))
            stopifnot(!v$visible, identical(v$value, x),
                      identical(getOption("digits"), 7L))
        }
    }
    cat("coercion and default:\n")
    print.default(pi, digits = "2")
    print(pi, digits = 2.8)
    print(pi, digits = NULL)
    print.digits_callback <- function(x, ...) {
        stopifnot(identical(getOption("digits"), 7L))
        cat("callback default, explicit, and format:\n")
        print(pi)
        print(pi, digits = 4)
        print(format(pi, digits = 3))
        invisible(gc())
        invisible(x)
    }
    assign("print.digits_callback", print.digits_callback, envir = .GlobalEnv)
    on.exit(rm("print.digits_callback", envir = .GlobalEnv), add = TRUE)
    obj <- structure(list(), class = "digits_callback")
    cat("recursive parameters restored after callback:\n")
    print(list(pi, obj, pi, expression(foo), pi), digits = 2)
    print.digits_error <- function(x, ...) {
        print(pi, digits = 4)
        invisible(gc())
        stop("print callback failed")
    }
    assign("print.digits_error", print.digits_error, envir = .GlobalEnv)
    on.exit(rm("print.digits_error", envir = .GlobalEnv), add = TRUE)
    failed <- tryCatch({
        print(list(pi, structure(list(), class = "digits_error")), digits = 2)
        FALSE
    }, error = function(e) TRUE)
    stopifnot(failed, identical(getOption("digits"), 7L))
    cat("default restored after callback error:\n")
    print(pi)
    obj_error <- structure(list(), class = "digits_error")
    cat("nested list and pairlist paths before caught errors:\n")
    for (x in list(list(pi, list(obj_error)),
                   pairlist(before = pi, nested = pairlist(fail = obj_error)))) {
        failed <- tryCatch({ print(x, digits = 2); FALSE },
                           error = function(e) TRUE)
        stopifnot(failed, identical(getOption("digits"), 7L))
    }
    print.digits_inline <- function(x, ...) cat("no newline")
    assign("print.digits_inline", print.digits_inline, envir = .GlobalEnv)
    on.exit(rm("print.digits_inline", envir = .GlobalEnv), add = TRUE)
    cat("method without a terminal newline:\n")
    print(list(structure(list(), class = "digits_inline"), pi), digits = 2)
    print.digits_stream <- function(x, ...) {
        cat("before ")
        message("method message")
        cat("after\n")
        invisible(gc())
        stop("stream callback failed")
    }
    assign("print.digits_stream", print.digits_stream, envir = .GlobalEnv)
    on.exit(rm("print.digits_stream", envir = .GlobalEnv), add = TRUE)
    cat("output and message sinks around a caught method error:\n")
    err <- capture.output({
        out <- capture.output(tryCatch(
            print(list(pi, structure(list(), class = "digits_stream")), digits = 2),
            error = function(e) cat("caught\n")))
    }, type = "message")
    writeLines(out)
    writeLines(err)
    stopifnot(identical(getOption("digits"), 7L))
    f <- compiler::cmpfun(function(x, digits) print(x, digits = digits))
    cat("compiled explicit digits:\n")
    f(c(pi, exp(1)), 2)
})
