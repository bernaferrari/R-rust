#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBSET=63, DOMISSING=30, DFLTSUBSET=64
# for missing-index forms: x[], x[,], and x[i,].
# optimize=3 keeps GETVAR; STARTSUBSET; DOMISSING/GETVAR_MISSOK+PUSHARG; DFLTSUBSET; RETURN.
# Present indices without a missing slot stay on STARTSUBSET_N / MATSUBSET / VECSUBSET.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dfltsubset"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
empty <- cmpfun(function(x) x[], options = opts)
matrix_missing <- cmpfun(function(x) x[,], options = opts)
row_missing <- cmpfun(function(x, i) x[i,], options = opts)
saveRDS(empty, file.path(dir, "empty.rds"), version = 2, compress = FALSE)
saveRDS(matrix_missing, file.path(dir, "matrix-missing.rds"), version = 2, compress = FALSE)
saveRDS(row_missing, file.path(dir, "row-missing.rds"), version = 2, compress = FALSE)
m <- matrix(1:6, 2, 3)
stopifnot(identical(empty(1:3), 1:3))
stopifnot(identical(matrix_missing(m), m))
stopifnot(identical(row_missing(m, 1L), c(1L, 3L, 5L)))
