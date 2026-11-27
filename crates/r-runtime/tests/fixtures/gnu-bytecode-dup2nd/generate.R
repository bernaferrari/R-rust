#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit DUP2ND=101 for compiled x$a[1] <- v.
# optimize=3 keeps GETVAR v; STARTASSIGN x; DUP2ND; DOLLAR; SWAP;
# STARTSUBASSIGN_N; VECSUBASSIGN; DOLLARGETS; ENDASSIGN; POP; GETVAR x;
# RETURN. Uncompressed XDR version 2 lets tests mutate the opcode without
# changing retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dup2nd"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
dup2nd <- cmpfun(function(x, v) { x$a[1L] <- v; x }, options = opts)
saveRDS(dup2nd, file.path(dir, "dup2nd.rds"), version = 2, compress = FALSE)
stopifnot(identical(dup2nd(list(a = c(1L, 2L)), 9L)$a, c(9L, 2L)))
