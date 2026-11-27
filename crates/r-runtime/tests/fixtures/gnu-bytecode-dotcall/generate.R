#!/usr/bin/env Rscript
# GNU 4.6.1 compiler emits DOTCALL=119 for unnamed .Call with <= 16 args.
# Uncompressed XDR v2 lets tests mutate the opcode without changing source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dotcall"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
saveRDS(
  cmpfun(function() .Call("rportDotcallZero"), options = opts),
  file.path(dir, "zero.rds"),
  version = 2,
  compress = FALSE
)
saveRDS(
  cmpfun(function(x) .Call("rportDotcallOne", x), options = opts),
  file.path(dir, "one.rds"),
  version = 2,
  compress = FALSE
)
