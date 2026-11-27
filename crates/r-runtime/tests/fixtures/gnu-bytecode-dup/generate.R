# GNU R oracle bac583951b728e97b9786804d3b4081f0fe18df5.
# DUP is opcode 5; execute the actual stream, not its retained source expression.
f <- function(x) NULL
body(f) <- .Internal(mkCode(as.integer(c(12,20,1,5,44,0,1)), list(quote(x+x),quote(x))))
stopifnot(identical(f(c(2L,NA_integer_)),c(4L,NA_integer_)))
saveRDS(f,"crates/r-embed/tests/fixtures/gnu-bytecode-dup/duplicate.rds",version=2,compress=FALSE)
