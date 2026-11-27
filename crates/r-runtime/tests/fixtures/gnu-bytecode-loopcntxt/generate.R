# Generate with Homebrew GNU R 4.6.1 from the repository root:
# /opt/homebrew/Cellar/r/4.6.1/bin/Rscript crates/r-embed/tests/fixtures/gnu-bytecode-loopcntxt/generate.R

out <- "crates/r-embed/tests/fixtures/gnu-bytecode-loopcntxt"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

saveRDS(compiler::cmpfun(function(x) {
    for (i in x) eval(quote(NULL))
    0L
}), file.path(out, "eval-for.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(x) {
    repeat {
        eval(quote(NULL))
        return(x)
    }
}), file.path(out, "eval-repeat.rds"), version = 2, compress = FALSE)
