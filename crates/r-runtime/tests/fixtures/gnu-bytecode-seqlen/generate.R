#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit SEQLEN=122 for seq_len(n).
# optimize=3 keeps GETVAR; SEQLEN call=0; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-seqlen"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
seqlen <- cmpfun(function(n) seq_len(n), options = opts)
saveRDS(seqlen, file.path(dir, "seqlen.rds"), version = 2, compress = FALSE)
stopifnot(identical(seqlen(3L), 1:3))
stopifnot(identical(seqlen(0), integer(0)))
