# Generate with the pinned GNU R r90451 oracle. The retained bytecode's
# generated numeric empty-alternative handler calls the shadowable stop;
# a returning stop leaves a discarded operand below the LDNULL result.
# Usage: Rscript --vanilla gnu-switch-unequal-stack.R output.rds
args <- commandArgs(trailingOnly = TRUE)
stopifnot(length(args) == 1L)
e <- new.env(parent = baseenv())
e$stop <- function(...) 99L
f <- eval(quote(function(x) { v <- switch(x, a = , b = 1L); v }), e)
f <- compiler::cmpfun(f)
stopifnot(is.null(f(1L)), is.null(f(99L)), identical(f(2L), 1L),
          identical(f("a"), 1L))
writeBin(serialize(f, NULL, version = 3), args[[1L]])
