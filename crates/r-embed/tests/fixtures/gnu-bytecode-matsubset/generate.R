#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBSET_N=104 and MATSUBSET=85 for x[i,j].
# x[i,j,k] emits SUBSET_N=112 with rank 3.
# optimize=3 keeps GETVAR; STARTSUBSET_N; LDCONST/GETVAR_MISSOK; MATSUBSET/SUBSET_N; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-matsubset"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
matsubset_const <- cmpfun(function(x) x[1L, 2L], options = opts)
matsubset <- cmpfun(function(x, i, j) x[i, j], options = opts)
subset_n <- cmpfun(function(x) x[1L, 2L, 3L], options = opts)
saveRDS(matsubset_const, file.path(dir, "matsubset-const.rds"), version = 2, compress = FALSE)
saveRDS(matsubset, file.path(dir, "matsubset.rds"), version = 2, compress = FALSE)
saveRDS(subset_n, file.path(dir, "subset-n.rds"), version = 2, compress = FALSE)
m <- matrix(1:6, 2, 3)
a <- array(1:24, c(2, 3, 4))
stopifnot(identical(matsubset_const(m), 3L))
stopifnot(identical(matsubset(m, 2L, 3L), 6L))
stopifnot(identical(subset_n(a), 15L))
