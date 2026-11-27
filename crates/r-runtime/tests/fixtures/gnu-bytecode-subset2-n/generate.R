#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBSET2_N=110 and SUBSET2_N=113
# for x[[i,j,k]]. optimize=3 keeps GETVAR; STARTSUBSET2_N; LDCONST x3;
# SUBSET2_N call=0 rank=3; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-subset2-n"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
subset2_n_const <- cmpfun(function(x) x[[1L, 2L, 3L]], options = opts)
subset2_n <- cmpfun(function(x, i, j, k) x[[i, j, k]], options = opts)
saveRDS(subset2_n_const, file.path(dir, "subset2-n-const.rds"), version = 2, compress = FALSE)
saveRDS(subset2_n, file.path(dir, "subset2-n.rds"), version = 2, compress = FALSE)
a <- array(1:24, c(2, 3, 4))
stopifnot(identical(subset2_n_const(a), 15L))
stopifnot(identical(subset2_n(a, 2L, 3L, 4L), 24L))
