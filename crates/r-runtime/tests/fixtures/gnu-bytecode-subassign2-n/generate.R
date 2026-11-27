#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN2_N=111 and
# SUBASSIGN2_N=115 for compiled x[[i,j,k]] <- v. Rank 2 stays on
# MATSUBASSIGN2. optimize=3 keeps GETVAR v; STARTASSIGN x;
# STARTSUBASSIGN2_N; LDCONST / GETVAR_MISSOK; SUBASSIGN2_N rank=3;
# ENDASSIGN; POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-subassign2-n"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
subassign2_n_const <- cmpfun(function(x, v) { x[[1L, 2L, 3L]] <- v; x }, options = opts)
subassign2_n <- cmpfun(function(x, i, j, k, v) { x[[i, j, k]] <- v; x }, options = opts)
saveRDS(subassign2_n_const, file.path(dir, "subassign2-n-const.rds"), version = 2, compress = FALSE)
saveRDS(subassign2_n, file.path(dir, "subassign2-n.rds"), version = 2, compress = FALSE)
a <- array(1:24, c(2, 3, 4))
stopifnot(identical(subassign2_n_const(a, 99L)[[1L, 2L, 3L]], 99L))
stopifnot(identical(subassign2_n(a, 2L, 3L, 4L, 8L)[[2L, 3L, 4L]], 8L))
