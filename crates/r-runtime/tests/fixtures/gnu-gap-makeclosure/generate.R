# Generate with Homebrew GNU R 4.6.1 from the repository root:
# /opt/homebrew/Cellar/r/4.6.1/bin/Rscript crates/r-embed/tests/fixtures/gnu-gap-makeclosure/generate.R
#
# R version 4.6.1 (2026-06-24)
# Uncompressed XDR v2 so tests can inspect the MAKECLOSURE stream.

out <- "crates/r-embed/tests/fixtures/gnu-gap-makeclosure"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

# MAKECLOSURE / SETVAR / GETFUN / PUSHCONSTARG / CALL / RETURN
saveRDS(compiler::cmpfun(function() { f <- function(x) x + 1; f(2) }),
        file.path(out, "nested.rds"), version = 2, compress = FALSE)

# Nested compiled closure used as a value, not immediately called.
saveRDS(compiler::cmpfun(function() { f <- function(x) x + 1; f }),
        file.path(out, "nested-return.rds"), version = 2, compress = FALSE)
