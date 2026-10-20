# Run with the pinned oracle/r-oracle.json GNU R executable from the repo root.
path <- "crates/r-embed/tests/fixtures/gnu-sequential-v2.rds.gz"
con <- gzfile(path, "wb")
invisible(serialize(7L, con, version = 2))
invisible(serialize(9L, con, version = 2))
writeBin(as.raw(c(71, 72)), con)
close(con)

env <- new.env()
fun <- function() NULL
body(fun) <- as.call(c(list(as.name("{"), env), as.list(1:100)))
fun <- compiler::cmpfun(fun)
compiled <- serialize(fun, NULL, version = 2, refhook = function(x) "token")
writeBin(compiled, "crates/r-embed/tests/fixtures/gnu-bytecode-stream-hooks-v2.rds")
source("crates/rmath/src/mainutils/serialize/compiled-hook-contract.R", print.eval = TRUE)
