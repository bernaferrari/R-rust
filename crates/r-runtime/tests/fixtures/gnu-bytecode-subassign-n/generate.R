#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN_N=105 and SUBASSIGN_N=114
# for compiled x[i,j,k] <- v. Rank 2 stays on MATSUBASSIGN.
# optimize=3 keeps GETVAR v; STARTASSIGN x; STARTSUBASSIGN_N; LDCONST /
# GETVAR_MISSOK; SUBASSIGN_N rank=3; ENDASSIGN; POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-subassign-n"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
subassign_n_const <- cmpfun(function(x, v) { x[1L, 2L, 3L] <- v; x }, options = opts)
subassign_n <- cmpfun(function(x, i, j, k, v) { x[i, j, k] <- v; x }, options = opts)
saveRDS(subassign_n_const, file.path(dir, "subassign-n-const.rds"), version = 2, compress = FALSE)
saveRDS(subassign_n, file.path(dir, "subassign-n.rds"), version = 2, compress = FALSE)
a <- array(1:24, c(2, 3, 4))
stopifnot(identical(subassign_n_const(a, 99L)[1L, 2L, 3L], 99L))
stopifnot(identical(subassign_n(a, 2L, 3L, 4L, 8L)[2L, 3L, 4L], 8L))
