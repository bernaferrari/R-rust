#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN2_N=111 and
# MATSUBASSIGN2=109 for compiled x[[i,j]] <- v. optimize=3 keeps
# GETVAR v; STARTASSIGN x; STARTSUBASSIGN2_N; LDCONST / GETVAR_MISSOK;
# MATSUBASSIGN2; ENDASSIGN; POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-matsubassign2"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
matsubassign2_const <- cmpfun(function(x, v) { x[[1L, 2L]] <- v; x }, options = opts)
matsubassign2 <- cmpfun(function(x, i, j, v) { x[[i, j]] <- v; x }, options = opts)
saveRDS(matsubassign2_const, file.path(dir, "matsubassign2-const.rds"), version = 2, compress = FALSE)
saveRDS(matsubassign2, file.path(dir, "matsubassign2.rds"), version = 2, compress = FALSE)
m <- matrix(1:6, 2, 3)
stopifnot(identical(matsubassign2_const(m, 99L)[1L, 2L], 99L))
stopifnot(identical(matsubassign2(m, 2L, 3L, 8L)[2L, 3L], 8L))
