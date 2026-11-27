trace <- character()
f <- function() { on.exit({trace <<- c(trace, "first"); return(99); trace <<- c(trace, "unreachable")}); 1 }
print(list(value=f(), trace=trace))
trace <- character()
f <- function() { on.exit({trace <<- c(trace,"first"); return(99)}, add=TRUE, after=TRUE); on.exit(trace <<- c(trace,"second"), add=TRUE, after=TRUE); 1 }
print(list(value=f(), trace=trace))
trace <- character()
f <- function() { on.exit({trace <<- c(trace,"first"); return(99)}, add=TRUE, after=TRUE); on.exit({trace <<- c(trace,"second"); return(101)}, add=TRUE, after=TRUE); 1 }
print(list(value=f(), trace=trace))
f <- function() {on.exit(return(invisible(99))); 1}; print(withVisible(f()))
f <- function() {on.exit(return(99)); invisible(1)}; print(withVisible(f()))
trace <- character()
outer <- function() withRestarts(inner(), done=function() "restarted")
inner <- function() {on.exit({trace <<- c(trace,"first"); return("from-onexit")}, add=TRUE, after=TRUE); on.exit(trace <<- c(trace,"second"), add=TRUE, after=TRUE); invokeRestart("done")}
print(list(value=outer(),trace=trace))
f <- compiler::cmpfun(function() {on.exit({return(99); stop("unreachable")}); 1}); print(f())
f <- function() {on.exit(return(local({99}))); 1}; print(f())
trace <- character()
f <- function() {on.exit({gc(); return(list(value=99, tag="owned"))}, add=TRUE, after=TRUE); on.exit({gc(); trace <<- c(trace,"collected")}, add=TRUE, after=TRUE); 1}
print(list(value=f(),trace=trace))
print(tryCatch((function() {on.exit(stop("exit-error")); return(1)})(), error=function(e) conditionMessage(e)))
print(1 + 1)
print(tryCatch((function() {on.exit(return(99)); stop("body-error")})(), error=function(e) conditionMessage(e)))
trace <- character()
f <- function() {on.exit(stop("first-error"), add=TRUE, after=TRUE); on.exit(trace <<- c(trace,"second"), add=TRUE, after=TRUE); 1}
print(tryCatch(f(),error=function(e) conditionMessage(e)));print(trace)
trace <- character()
f <- function() {on.exit(stop("first-error"), add=TRUE, after=TRUE); on.exit(return(99), add=TRUE, after=TRUE); on.exit(trace <<- c(trace,"third"), add=TRUE, after=TRUE); 1}
print(tryCatch(f(),error=function(e) conditionMessage(e)));print(trace)
