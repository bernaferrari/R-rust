#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBSET2_N=110 and MATSUBSET2=107
# for x[[i,j]]. optimize=3 keeps GETVAR; STARTSUBSET2_N; LDCONST;
# MATSUBSET2; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-matsubset2"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
matsubset2 <- cmpfun(function(x) x[[1L, 1L]], options = opts)
saveRDS(matsubset2, file.path(dir, "matsubset2.rds"), version = 2, compress = FALSE)
m <- matrix(1:6, 2, 3)
stopifnot(identical(matsubset2(m), 1L))
