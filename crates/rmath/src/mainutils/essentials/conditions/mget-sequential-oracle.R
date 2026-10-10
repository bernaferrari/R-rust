for (first in c(TRUE, FALSE)) {
  e <- new.env()
  count <- 0L
  delayedAssign("x", { count <<- count + 1L; 42L }, assign.env = e)
  requested <- if (first) c("", "x") else c("x", "")
  message <- tryCatch(mget(requested, e, ifnotfound = list(7L)), error = function(e) conditionMessage(e))
  expected <- if (first) "invalid name in position 1" else "attempt to use zero-length variable name"
  stopifnot(identical(message, expected), identical(count, if (first) 0L else 1L))
}
count <- 0L
e <- new.env()
message <- tryCatch(mget(c("missing", ""), e, ifnotfound = list(function(name) { count <<- count + 1L; 9L })), error = function(e) conditionMessage(e))
stopifnot(identical(message, "attempt to use zero-length variable name"), identical(count, 1L))
message <- tryCatch(mget(c("x", ""), NULL, mode = 1L), error = function(e) conditionMessage(e))
stopifnot(identical(message, "use of NULL environment is defunct"))
message <- tryCatch(mget(c("x", ""), e, mode = c("any", "any", "any")), error = function(e) conditionMessage(e))
stopifnot(identical(message, "wrong length for 'mode' argument"))
cat("first/later names, promise/fallback effects and admission order=PASS\n")
TRUE
