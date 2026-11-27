#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit LOG=116 for log(x).
# optimize=3 keeps GETVAR; LOG call=0; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-log"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
logf <- cmpfun(function(x) log(x), options = opts)
saveRDS(logf, file.path(dir, "log.rds"), version = 2, compress = FALSE)
stopifnot(isTRUE(all.equal(logf(c(1, exp(1))), c(0, 1))))

