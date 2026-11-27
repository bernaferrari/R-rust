#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN_N=105 and MATSUBASSIGN=87
# for compiled x[i,j] <- v. Missing columns/rows stay on STARTSUBASSIGN /
# DFLTSUBASSIGN; drop= named args also stay on that default path.
# optimize=3 keeps GETVAR v; STARTASSIGN x; STARTSUBASSIGN_N; LDCONST /
# GETVAR_MISSOK; MATSUBASSIGN; ENDASSIGN; POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-matsubassign"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
matsubassign_const <- cmpfun(function(x, v) { x[1L, 2L] <- v; x }, options = opts)
matsubassign <- cmpfun(function(x, i, j, v) { x[i, j] <- v; x }, options = opts)
saveRDS(matsubassign_const, file.path(dir, "matsubassign-const.rds"), version = 2, compress = FALSE)
saveRDS(matsubassign, file.path(dir, "matsubassign.rds"), version = 2, compress = FALSE)
m <- matrix(1:6, 2, 3)
stopifnot(identical(matsubassign_const(m, 9L)[1L, 2L], 9L))
stopifnot(identical(matsubassign(m, 2L, 3L, 8L)[2L, 3L], 8L))
