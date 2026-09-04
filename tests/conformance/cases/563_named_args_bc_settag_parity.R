# Named arguments in compiled closure bodies bind by name (eval.c SETTAG):
# the bytecode call opcodes rebuild argument cells and must carry the tag.
# Regression: paste(x, collapse=",") inside a function body lost the tag,
# making "," a positional argument ("World ," instead of "World").
f <- function(x) paste(x, collapse = ",")
cat("collapse:", f("World"), "\n")
g <- function(x) paste(x, collapse = ", ")
cat("collapse-vec:", g(c("a", "b")), "\n")
h <- function(x) paste(x, sep = "-")
cat("sep:", h("a"), "\n")
k <- function(x, y) paste(x, y, sep = "/")
cat("two-args:", k("a", "b"), "\n")
cat("toplevel:", paste("World", collapse = ","), "\n")
