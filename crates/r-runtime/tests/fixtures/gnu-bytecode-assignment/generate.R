#!/usr/bin/env Rscript
# Regenerate with the pinned GNU R oracle:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-assignment/generate.R

library(compiler)
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-assignment"
dir.create(out, recursive = TRUE, showWarnings = FALSE)
f <- compiler::cmpfun(function(x) {
  y <- x
  y
})
saveRDS(f, file.path(out, "setvar.rds"), version = 2, compress = FALSE)
f <- compiler::cmpfun(function(x) {
  y <<- x
  y
})
saveRDS(f, file.path(out, "setvar2.rds"), version = 2, compress = FALSE)
