# Generated with the R oracle pinned in oracle/r-oracle.json.
# Keep serialization at version 2 and uncompressed so byte mutations in the
# adapter boundary tests identify one exact big-endian instruction stream.
library(compiler)

fixtures <- list(
    identity = cmpfun(function(x) x),
    branch = cmpfun(function(x) if (x) 1L else 2L),
    noelse = cmpfun(function(x) if (x) 1L),
    unbound = cmpfun(function() gnu_adapter_unbound)
)

for (name in names(fixtures)) {
    saveRDS(
        fixtures[[name]],
        file.path("crates/r-embed/tests/fixtures/gnu-bytecode-branches",
                  paste0(name, ".rds")),
        version = 2,
        compress = FALSE
    )
}
