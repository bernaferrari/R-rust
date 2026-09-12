#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN2_N=111 and VECSUBASSIGN2=108
# for compiled x[[i]] <- v. optimize=3 keeps GETVAR v; STARTASSIGN x;
# STARTSUBASSIGN2_N; GETVAR_MISSOK i; VECSUBASSIGN2; ENDASSIGN; POP; GETVAR x;
# RETURN. Uncompressed XDR version 2 lets tests mutate the opcode without
# changing retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-vecsubassign2"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
vecsubassign2 <- cmpfun(function(x, i, v) { x[[i]] <- v; x }, options = opts)
saveRDS(vecsubassign2, file.path(dir, "vecsubassign2.rds"), version = 2, compress = FALSE)
stopifnot(identical(vecsubassign2(list(a = 1L, b = 2L), 2L, 9L)[[2]], 9L))
stopifnot(identical(vecsubassign2(c(1L, 2L), 1L, 8L), c(8L, 2L)))
