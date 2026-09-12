# Generate with the pinned trunk oracle from the repository root:
# ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-dots/generate.R
#
# Default-optimize (2) compilation emits DDVAL, DODOTS, MAKECLOSURE, and
# CALLSPECIAL.  Uncompressed XDR version 2 lets tests find the instruction
# stream by byte pattern.

out <- "crates/r-embed/tests/fixtures/gnu-bytecode-dots"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

saveRDS(compiler::cmpfun(function(x, ...) c(sum(x), ..1)),
        file.path(out, "ddval.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(a, ...) sum(...)),
        file.path(out, "dodots.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(x) { g <- function(y) y + 1; g(x) }),
        file.path(out, "makeclosure.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(x) `=`(a, 1) + x),
        file.path(out, "callspecial.rds"), version = 2, compress = FALSE)
