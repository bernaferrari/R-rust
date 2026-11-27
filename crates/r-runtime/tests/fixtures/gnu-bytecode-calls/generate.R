# Run with the pinned GNU oracle in oracle/r-oracle.json, from the repo root.
root <- 'crates/r-embed/tests/fixtures/gnu-bytecode-calls'
saveRDS(compiler::cmpfun(function(x) identity(x)), file.path(root, 'identity-promise.rds'), version=2, compress=FALSE)
saveRDS(compiler::cmpfun(function(x, y) target(x, y)), file.path(root, 'pair-promise.rds'), version=2, compress=FALSE)
saveRDS(compiler::cmpfun(function(x) 1 + identity(x)), file.path(root, 'call-add.rds'), version=2, compress=FALSE)
