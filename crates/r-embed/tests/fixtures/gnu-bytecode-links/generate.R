# Generate with the pinned trunk oracle from the repository root:
# ~/.cache/rport/r-oracle/bac583951b728e97b9786804d3b4081f0fe18df5/bin/Rscript \
#   crates/r-embed/tests/fixtures/gnu-bytecode-links/generate.R

out <- "crates/r-embed/tests/fixtures/gnu-bytecode-links"
dir.create(out, recursive = TRUE, showWarnings = FALSE)

saveRDS(compiler::cmpfun(function(x) (x + 1)),
        file.path(out, "visible.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(x) c(x$a <- 1, 2)),
        file.path(out, "incnkstk.rds"), version = 2, compress = FALSE)

saveRDS(compiler::cmpfun(function(x) .Internal(Sys.getpid())),
        file.path(out, "intlbuiltin.rds"), version = 2, compress = FALSE)
