# Stock match.c unused-argument wording: every leftover argument is listed,
# singular with one, plural with several, named args as "tag = value".

f <- function(a, b) a
g <- function(x) x
cat("one-pos:", tryCatch(f(1, 2, 3), error = function(e) conditionMessage(e)), "\n")
cat("named-one:", tryCatch(g(y = 1), error = function(e) conditionMessage(e)), "\n")
cat("multi-named:", tryCatch({h <- function(a) a; h(x = 1, y = 2)},
  error = function(e) conditionMessage(e)), "\n")
cat("multi-pos:", tryCatch(f(1, 2, 3, 4), error = function(e) conditionMessage(e)), "\n")
cat("multi-named2:", tryCatch(f(a = 1, b = 2, c = 3, d = 4),
  error = function(e) conditionMessage(e)), "\n")
cat("expr-arg:", tryCatch(f(1, 2, x + 2), error = function(e) conditionMessage(e)), "\n")
