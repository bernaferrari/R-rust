# Generate with Homebrew GNU R 4.6.1 from the repository root:
# /opt/homebrew/Cellar/r/4.6.1/bin/Rscript crates/r-embed/tests/fixtures/gnu-gap-returnjmp/generate.R
#
# A loop body containing eval() forces STARTLOOPCNTXT and needRETURNJMP.

out <- "crates/r-embed/tests/fixtures/gnu-gap-returnjmp"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

saveRDS(compiler::cmpfun(function(x) {
    for (i in x) {
        eval(quote(NULL))
        if (i > 0L) return(i)
    }
    0L
}), file.path(out, "returnjmp-eval-loop.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(x) {
    repeat {
        eval(quote(NULL))
        return(x)
    }
}), file.path(out, "returnjmp-repeat.rds"), version = 2, compress = FALSE)
