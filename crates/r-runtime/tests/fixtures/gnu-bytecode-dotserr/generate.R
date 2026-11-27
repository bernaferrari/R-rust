#!/usr/bin/env Rscript
# GNU 4.6.1 compiler emits DOTSERR=60 for `...` used outside a dots context.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dotserr"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
saveRDS(
  cmpfun(function() ..., options = list(optimize = 3L)),
  file.path(dir, "dotserr.rds"),
  version = 2,
  compress = FALSE
)
