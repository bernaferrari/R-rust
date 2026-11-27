#!/usr/bin/env Rscript
# Regenerate with the pinned GNU R oracle:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-special-constant-args/generate.R

library(compiler)
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-special-constant-args"
dir.create(out, recursive = TRUE, showWarnings = FALSE)
saveRDS(cmpfun(function() target(NULL)), file.path(out, "pushnullarg.rds"),
        version = 2, compress = FALSE)
saveRDS(cmpfun(function() target(TRUE)), file.path(out, "pushtruearg.rds"),
        version = 2, compress = FALSE)
saveRDS(cmpfun(function() target(FALSE)), file.path(out, "pushfalsearg.rds"),
        version = 2, compress = FALSE)
