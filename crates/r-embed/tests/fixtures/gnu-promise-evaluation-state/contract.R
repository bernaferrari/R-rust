# Independent GNU revision 90451 promise-state observations.
capture <- function(expr) {
    warnings <- character()
    result <- withCallingHandlers(
        tryCatch(force(expr), error=function(e) list(error=conditionMessage(e), call=conditionCall(e))),
        warning=function(w) { warnings <<- c(warnings, conditionMessage(w)); invokeRestart('muffleWarning') }
    )
    list(result=result, warnings=warnings)
}
f <- function(x=x) x
recursive.default <- capture(f())
delayedAssign('.self', .self, assign.env=.GlobalEnv, eval.env=.GlobalEnv)
recursive.delayed <- capture(.self)
count <- 0L
delayedAssign('.retry', { count <<- count + 1L; if (count == 1L) stop('first'); 37L }, assign.env=.GlobalEnv, eval.env=.GlobalEnv)
retry.first <- capture(.retry)
retry.second <- capture(.retry)
retry.third <- capture(.retry)
stopifnot(
    identical(recursive.default$result$error, 'promise already under evaluation: recursive default argument reference or earlier problems?'),
    identical(recursive.delayed$result$error, recursive.default$result$error),
    identical(recursive.default$result$call, quote(f())),
    identical(recursive.delayed$result$call, quote(force(expr))),
    identical(retry.first$result$error, 'first'),
    identical(retry.second$result, 37L),
    identical(retry.second$warnings, 'restarting interrupted promise evaluation'),
    identical(retry.third$result, 37L), length(retry.third$warnings) == 0L,
    identical(count, 2L)
)
dput(list(recursive.default=recursive.default, recursive.delayed=recursive.delayed, retry.first=retry.first, retry.second=retry.second, retry.third=retry.third, count=count))
cat('GNU promise state contract PASS\n')
count <- 0L
delayedAssign('.warn.retry', {
    count <<- count + 1L
    if (count == 1L) stop('first')
    37L
})
first <- tryCatch(.warn.retry, error=conditionMessage)
nested <- NULL
second <- withCallingHandlers(.warn.retry, warning=function(w) {
    nested <<- tryCatch(.warn.retry, error=conditionMessage)
    invokeRestart('muffleWarning')
})
stopifnot(
    identical(first, 'first'), identical(second, 37L), identical(count, 2L),
    identical(nested, recursive.default$result$error)
)
count2 <- 0L
delayedAssign('.warn.error', {
    count2 <<- count2 + 1L
    if (count2 == 1L) stop('first')
    41L
})
first <- tryCatch(.warn.error, error=conditionMessage)
options(warn=2L)
second <- tryCatch(.warn.error, error=conditionMessage)
stopifnot(
    identical(first, 'first'), identical(count2, 1L),
    identical(second, '(converted from warning) restarting interrupted promise evaluation')
)
options(warn=0L)
third <- tryCatch(suppressWarnings(.warn.error), error=conditionMessage)
stopifnot(identical(third, recursive.default$result$error), identical(count2, 1L))
dput(list(reentry=nested, restart.warning.error=second, later.force=third, counter=count2))
cat('GNU warning reentry and warning-error promise state contract PASS\n')
