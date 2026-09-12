# Generate with Homebrew GNU R 4.6.1 from the repository root:
# /opt/homebrew/Cellar/r/4.6.1/bin/Rscript crates/r-embed/tests/fixtures/gnu-gap-c/generate.R
#
# GNU 4.6.1 no longer emits STARTC/DFLTC (cmpDispatch is commented out in
# compiler/R/cmp.R). c(...) still emits DODOTS.OP.

out <- "crates/r-embed/tests/fixtures/gnu-gap-c"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

saveRDS(compiler::cmpfun(function(...) c(...)),
        file.path(out, "c-dots.rds"), version = 2, compress = FALSE)
