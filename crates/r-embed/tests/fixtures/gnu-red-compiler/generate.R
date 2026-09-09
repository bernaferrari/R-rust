# Generate with the pinned GNU R oracle from the repository root:
# /Users/bernardoferrari/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript crates/r-embed/tests/fixtures/gnu-red-compiler/generate.R

out <- "crates/r-embed/tests/fixtures/gnu-red-compiler"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

f <- compiler::cmpfun(function(x) sqrt(x))
saveRDS(f, file.path(out, "sqrt.rds"), version = 2, compress = FALSE)

f <- compiler::cmpfun(function(x) abs(x))
saveRDS(f, file.path(out, "abs.rds"), version = 2, compress = FALSE)

f <- compiler::cmpfun(function(x) {
    s <- 0L
    for (i in x) {
        if (i < 0L) next
        s <- s + i
    }
    s
})
saveRDS(f, file.path(out, "loop.rds"), version = 2, compress = FALSE)
