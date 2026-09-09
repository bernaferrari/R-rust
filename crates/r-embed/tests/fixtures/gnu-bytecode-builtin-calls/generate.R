#!/usr/bin/env Rscript
# Regenerate with the pinned GNU R oracle:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-builtin-calls/generate.R

library(compiler)
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-builtin-calls"
dir.create(out, recursive = TRUE, showWarnings = FALSE)
f <- compiler::cmpfun(function(x) abs(x))
saveRDS(f, file.path(out, "abs.rds"), version = 2, compress = FALSE)
saveRDS(compiler::cmpfun(function(x) abs(abs(x))), file.path(out, "nested.rds"), version=2, compress=FALSE)
saveRDS(compiler::cmpfun(function(x,y) abs(x)+abs(y)), file.path(out, "residual.rds"), version=2, compress=FALSE)
