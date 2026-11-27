local({
    for (name in c("suppressWarnings", "suppressMessages")) {
        f <- get(name, baseenv(), inherits = FALSE)
        stopifnot(is.function(f), identical(environment(f), asNamespace("base")),
                  identical(names(formals(f)), c("expr", "classes")),
                  !exists(name, .GlobalEnv, inherits = FALSE))
    }
    exercise <- function() {
        seen <- character()
        value <- suppressWarnings(withCallingHandlers({ warning("careful"); 7L },
            warning = function(w) { seen <<- c(seen, conditionMessage(w)); gc() }))
        stopifnot(identical(value, 7L), identical(seen, "careful"))
        seen <- character()
        withCallingHandlers(suppressWarnings(warning("hidden")),
            warning = function(w) seen <<- c(seen, conditionMessage(w)))
        stopifnot(identical(seen, character()))
        custom <- warningCondition("classed", class = "warningProbe", token = 42L)
        observed <- NULL
        suppressWarnings(withCallingHandlers(warning(custom), warningProbe = function(w) {
            observed <<- w; gc()
        }), classes = "warningProbe")
        stopifnot(identical(observed, custom))
        seen <- character()
        withCallingHandlers(suppressWarnings(warning(custom), classes = "otherProbe"),
            warning = function(w) { seen <<- c(seen, conditionMessage(w)); invokeRestart("muffleWarning") })
        stopifnot(identical(seen, "classed"))
        seen <- character()
        suppressWarnings(suppressWarnings(withCallingHandlers(warning("nested"),
            warning = function(w) { seen <<- c(seen, conditionMessage(w)); gc() }),
            classes = "otherProbe"))
        stopifnot(identical(seen, "nested"))
        seen <- character()
        suppressMessages(withCallingHandlers(message("hello"), message = function(m) {
            seen <<- c(seen, conditionMessage(m)); gc()
        }))
        stopifnot(identical(seen, "hello\n"))
        seen <- character()
        withCallingHandlers(suppressMessages(message("hidden")),
            message = function(m) seen <<- c(seen, conditionMessage(m)))
        stopifnot(identical(seen, character()))
        observed <- NULL
        warn <- function() warning("attributed")
        withCallingHandlers(warn(), warning = function(w) {
            observed <<- w; gc(); invokeRestart("muffleWarning")
        })
        stopifnot(identical(conditionCall(observed), quote(warn())))
        withCallingHandlers(compiler::cmpfun(warn)(), warning = function(w) {
            observed <<- w; gc(); invokeRestart("muffleWarning")
        })
        stopifnot(identical(conditionCall(observed), quote(compiler::cmpfun(warn)())))
        old <- options(warn = 2)
        on.exit(options(old))
        stopifnot(identical(suppressWarnings({ warning("converted"); 8L }), 8L))
        TRUE
    }
    stopifnot(exercise(), compiler::cmpfun(exercise)())
    stopifnot(identical(suppressWarnings(9L), 9L), identical(suppressMessages(10L), 10L))
    TRUE
})
