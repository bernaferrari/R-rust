#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBSET_N=104 and VECSUBSET=84 for x[i].
# optimize=3 keeps GETVAR; STARTSUBSET_N; GETVAR_MISSOK/LDCONST; VECSUBSET; RETURN.
# x[[i]] is STARTSUBSET2_N=110 / VECSUBSET2=106.
# Assignment is STARTASSIGN; STARTSUBASSIGN_N=105; GETVAR_MISSOK; VECSUBASSIGN=86; ENDASSIGN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-subset"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
subset <- cmpfun(function(x, i) x[i], options = opts)
subset_const <- cmpfun(function(x) x[1L], options = opts)
subset2 <- cmpfun(function(x, i) x[[i]], options = opts)
subassign <- cmpfun(function(x, i, v) { x[i] <- v; x }, options = opts)
saveRDS(subset, file.path(dir, "subset.rds"), version = 2, compress = FALSE)
saveRDS(subset_const, file.path(dir, "subset-const.rds"), version = 2, compress = FALSE)
saveRDS(subset2, file.path(dir, "subset2.rds"), version = 2, compress = FALSE)
saveRDS(subassign, file.path(dir, "subassign.rds"), version = 2, compress = FALSE)
stopifnot(identical(subset(1:3, 2L), 2L))
stopifnot(identical(subset_const(c(7L, 8L)), 7L))
stopifnot(identical(subset2(list(a = 9L), 1L), 9L))
stopifnot(identical(subassign(c(1L, 2L), 1L, 8L), c(8L, 2L)))
