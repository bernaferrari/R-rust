#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTASSIGN2=96 and ENDASSIGN2=97
# for super-assignment of $ and [ forms. Simple x <<- v stays SETVAR2.
# optimize=3 keeps LDCONST; STARTASSIGN2; DOLLARGETS/VECSUBASSIGN; ENDASSIGN2.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-assign2"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
dollar <- cmpfun(function() { x$a <<- 1L; x }, options = opts)
subset <- cmpfun(function(i) { x[i] <<- 8L; x }, options = opts)
saveRDS(dollar, file.path(dir, "dollar.rds"), version = 2, compress = FALSE)
saveRDS(subset, file.path(dir, "subset.rds"), version = 2, compress = FALSE)
dump <- function(fun, name) {
  bc <- compiler:::disassemble(fun)
  code <- as.integer(unlist(lapply(bc[[2]], function(x) if (is.numeric(x)) x else NA)))
  cat(name, "\n")
  print(bc[[2]])
  cat("ints:", paste(code[!is.na(code)], collapse = ","), "\n\n")
}
dump(dollar, "dollar")
dump(subset, "subset")
x <- list(a = 0L)
stopifnot(identical(dollar(), list(a = 1L)), identical(x, list(a = 1L)))
x <- c(1L, 2L)
stopifnot(identical(subset(1L), c(8L, 2L)), identical(x, c(8L, 2L)))
