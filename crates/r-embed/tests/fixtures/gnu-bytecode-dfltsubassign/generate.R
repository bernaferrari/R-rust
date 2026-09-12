#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN=65, DOMISSING=30,
# DFLTSUBASSIGN=66 for missing-index replacement: x[] <- v, x[,] <- v,
# and x[i,] <- v.
# optimize=3 keeps GETVAR v; STARTASSIGN x; STARTSUBASSIGN; DOMISSING /
# GETVAR_MISSOK+PUSHARG; DFLTSUBASSIGN; ENDASSIGN; POP; GETVAR x; RETURN.
# Present indices without a missing slot stay on STARTSUBASSIGN_N /
# VECSUBASSIGN / MATSUBASSIGN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dfltsubassign"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
empty <- cmpfun(function(x, v) { x[] <- v; x }, options = opts)
matrix_missing <- cmpfun(function(x, v) { x[,] <- v; x }, options = opts)
row_missing <- cmpfun(function(x, i, v) { x[i,] <- v; x }, options = opts)
saveRDS(empty, file.path(dir, "empty.rds"), version = 2, compress = FALSE)
saveRDS(matrix_missing, file.path(dir, "matrix-missing.rds"), version = 2, compress = FALSE)
saveRDS(row_missing, file.path(dir, "row-missing.rds"), version = 2, compress = FALSE)
stopifnot(identical(empty(c(1L, 2L, 3L), 9L), c(9L, 9L, 9L)))
m <- matrix(1:6, 2, 3)
stopifnot(identical(matrix_missing(m, 0L), matrix(0L, 2, 3)))
stopifnot(identical(row_missing(m, 1L, 8L), rbind(c(8L, 8L, 8L), c(2L, 4L, 6L))))
