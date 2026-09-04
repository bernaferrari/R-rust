# Bare `return()` returns NULL (missing argument, not a parse error).
f <- function() { return() }
cat("bare-return:", is.null(f()), "\n")
g <- function(x) { if (x) { return() }; 1 }
cat("branch-return:", g(FALSE), "\n")
