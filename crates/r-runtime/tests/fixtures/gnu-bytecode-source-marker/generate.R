# Generated with oracle/r-oracle.json, GNU R bac583951b728e97b9786804d3b4081f0fe18df5.
# Run from repository root with the pinned Rscript.
f <- compiler::cmpfun(function(x) {
    if (FALSE) C_modelframe
    if (x) 41L else 42L
})
saveRDS(f, "crates/r-embed/tests/fixtures/gnu-bytecode-source-marker/branch.rds",
        version = 2, compress = FALSE)
stopifnot(identical(f(TRUE), 41L), identical(f(FALSE), 42L))
