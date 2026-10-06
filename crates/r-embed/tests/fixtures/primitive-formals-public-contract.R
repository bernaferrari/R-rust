seen <- list()
for (fn in list(invisible, sum, c, `[`, sin, length)) {
    value <- withCallingHandlers(formals(fn), warning = function(w) {
        seen[[length(seen) + 1L]] <<- w
        invokeRestart("muffleWarning")
    })
    stopifnot(is.null(value), is.null(body(fn)))
}
stopifnot(length(seen) == 0L)
stopifnot(identical(formals(args(invisible)), pairlist(x = NULL)))
stopifnot(identical(formals(function(x, y = 2, ...) NULL), as.pairlist(alist(x = , y = 2, ... = ))))
TRUE
