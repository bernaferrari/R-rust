#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit LOGBASE=117 for log(x, 10).
# optimize=3 keeps GETVAR; LDCONST 10; LOGBASE call=0; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-logbase"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
logbase <- cmpfun(function(x) log(x, 10), options = opts)
saveRDS(logbase, file.path(dir, "logbase.rds"), version = 2, compress = FALSE)
logbase_var <- cmpfun(function(x, b) log(x, b), options = opts)
saveRDS(logbase_var, file.path(dir, "logbase-var.rds"), version = 2, compress = FALSE)
stopifnot(identical(logbase(c(1, 10, 100)), c(0, 1, 2)))
stopifnot(identical(logbase_var(100, 10), 2))
