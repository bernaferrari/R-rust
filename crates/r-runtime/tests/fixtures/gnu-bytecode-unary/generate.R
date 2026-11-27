# Oracle pinned in oracle/r-oracle.json; uncompressed XDR version 2.
fixtures <- list(negative=function(x)-x, positive=function(x)+x, power=function(x,y)x^y)
for (name in names(fixtures)) {
  saveRDS(compiler::cmpfun(fixtures[[name]]),
    file.path('crates/r-embed/tests/fixtures/gnu-bytecode-unary',paste0(name,'.rds')),
    version=2,compress=FALSE)
}
