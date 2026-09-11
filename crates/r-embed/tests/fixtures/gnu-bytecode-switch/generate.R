# Oracle commit bac583951b728e97b9786804d3b4081f0fe18df5; run from repository root.
f <- compiler::cmpfun(function(x) switch(x, a = 1L, b = 2L, 0L))
# Generated with the pinned GNU R oracle; keep the stream uncompressed so the
# integration test can mutate the imported instruction words directly.
saveRDS(
    f,
    "crates/r-embed/tests/fixtures/gnu-bytecode-switch/switch.rds",
    version = 2,
    compress = FALSE
)
