#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit SEQALONG=121 for seq_along(x).
# optimize=3 keeps GETVAR; SEQALONG call=0; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-seqalong"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
seqalong <- cmpfun(function(x) seq_along(x), options = opts)
saveRDS(seqalong, file.path(dir, "seqalong.rds"), version = 2, compress = FALSE)
stopifnot(identical(seqalong(letters[1:3]), 1:3))
stopifnot(identical(seqalong(list()), integer(0)))
