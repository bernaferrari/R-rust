#!/usr/bin/env Rscript
# Regenerate with the pinned GNU R oracle:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-named-calls/generate.R

library(compiler)
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-named-calls"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

save_fixture <- function(name, body) {
  saveRDS(compiler::cmpfun(body), file.path(out, name), version = 2, compress = FALSE)
}

save_fixture("reversed-tags.rds", function(x, y) target(b = y, a = x))
save_fixture("nested-reversed-tags.rds", function(x, y) target(b = identity(y), a = identity(x)))
