events <- character()
n <- 0L
f <- function(x) { n <<- n + 1L; events <<- c(events, "outer-eval"); if (n == 1L) NaN else x*x }
g <- function(x) { events <<- c(events, "inner-eval"); if (sum(events == "inner-eval") == 1L) Inf else x*x }
invisible(withCallingHandlers(optimize(f, c(-1, 1)), warning=function(w) {
    events <<- c(events, paste0("outer:", conditionMessage(w)))
    gc()
    invisible(withCallingHandlers(optimize(g, c(-1, 1)), warning=function(v) {
        events <<- c(events, paste0("inner:", conditionMessage(v)))
        invokeRestart("muffleWarning")
    }))
    invokeRestart("muffleWarning")
}))
cat(paste(events[1:4], collapse="|"), "\n", sep="")
cat(sum(grepl("^outer:", events)), " ", sum(grepl("^inner:", events)), "\n", sep="")
events <- character(); n <- 0L
old <- options(warn=2)
err <- tryCatch(optimize(function(x) { n <<- n+1L; NaN }, c(-1, 1)), error=function(e) conditionMessage(e))
options(old)
cat(n, " ", grepl("NA/NaN replaced", err, fixed=TRUE), "\n", sep="")
n <- 0L; warnings <- character()
invisible(withCallingHandlers(uniroot(function(x) { n <<- n+1L; if (n == 1L) NaN else x }, c(-1, 1), f.lower=-1, f.upper=1), warning=function(w) { warnings <<- c(warnings, conditionMessage(w)); invokeRestart("muffleWarning") }))
cat(length(warnings), " ", warnings[1], "\n", sep="")
n <- 0L; warnings <- character()
invisible(withCallingHandlers(optimize(function(x) { n <<- n+1L; if (n == 1L) -Inf else x*x }, c(-1,1)), warning=function(w) { warnings <<- c(warnings, conditionMessage(w)); invokeRestart("muffleWarning") }))
cat(length(warnings), " ", warnings[1], "\n", sep="")
n <- 0L; warnings <- character()
invisible(withCallingHandlers(optimize(function(x) { n <<- n+1L; if (n <= 2L) NaN else x*x }, c(-1,1)), warning=function(w) { warnings <<- c(warnings, conditionMessage(w)); invokeRestart("muffleWarning") }))
cat(length(warnings), " ", all(warnings == "NA/NaN replaced by maximum positive value"), "\n", sep="")
