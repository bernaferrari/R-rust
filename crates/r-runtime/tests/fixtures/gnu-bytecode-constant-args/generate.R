#!/usr/bin/env Rscript
# Regenerate with the pinned GNU R oracle:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-constant-args/generate.R

library(compiler)
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-constant-args"
dir.create(out, recursive = TRUE, showWarnings = FALSE)
f <- compiler::cmpfun(function(x) target(1L, x))
saveRDS(f, file.path(out, "pushconstarg.rds"), version = 2, compress = FALSE)
