#!/usr/bin/env Rscript
# Regenerate with the pinned GNU R oracle:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-checkfun/generate.R

library(compiler)
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-checkfun"
dir.create(out, recursive = TRUE, showWarnings = FALSE)
f <- compiler::cmpfun(function(f, x) (f)(x))
saveRDS(f, file.path(out, "callable-argument.rds"), version = 2, compress = FALSE)
# Retained failing namespace-call probe, tracked separately in rport-tte8.
saveRDS(compiler::cmpfun(function(x) base::abs(x)),
        file.path(out, "base-abs.rds"), version = 2, compress = FALSE)
