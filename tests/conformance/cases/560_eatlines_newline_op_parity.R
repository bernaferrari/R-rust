# EatLines: a newline before a binary operator continues the expression.
cat("nl-plus:", (1
 + 2), "\n")
cat("nl-or:", (TRUE
 || FALSE), "\n")
cat("nl-fnbody:", (function(e) {
 if (e)
 1
 else
 2
 })(TRUE), "\n")
