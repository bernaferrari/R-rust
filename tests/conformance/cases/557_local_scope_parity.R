# `local()` evaluates in a fresh child env: assignments stay local.
e <- local({ a <- 99; environment() })
cat("leak:", exists("a"), "\n")
cat("child-has:", exists("a", envir = e), "\n")
cat("value:", local({ b <- 1; b + 1 }), "\n")
