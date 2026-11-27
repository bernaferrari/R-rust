#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit COLON=120 for 1:n and a:b.
# optimize=3 constant-folds 1:5 to LDCONST of the integer sequence.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-colon"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
colon <- cmpfun(function(n) 1:n, options = opts)
colon_const <- cmpfun(function() 1:5, options = opts)
colon_range <- cmpfun(function(a, b) a:b, options = opts)
saveRDS(colon, file.path(dir, "colon.rds"), version = 2, compress = FALSE)
saveRDS(colon_const, file.path(dir, "colon-const.rds"), version = 2, compress = FALSE)
saveRDS(colon_range, file.path(dir, "colon-range.rds"), version = 2, compress = FALSE)
stopifnot(identical(colon(5L), 1:5))
stopifnot(identical(colon_const(), 1:5))
stopifnot(identical(colon_range(2L, 4L), 2:4))
