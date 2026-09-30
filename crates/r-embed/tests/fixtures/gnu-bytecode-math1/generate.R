#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit MATH1=118 for sin(x), expm1(x), and floor(x).
# math1funs[] index 6 is sin; index 3 is expm1; index 0 is floor.
# abs(x) is CALLBUILTIN and sqrt(x) is SQRT, not MATH1.
# optimize=3 keeps GETVAR; MATH1 call=0, fun; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-math1"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
sinf <- cmpfun(function(x) sin(x), options = opts)
expm1f <- cmpfun(function(x) expm1(x), options = opts)
floorf <- cmpfun(function(x) floor(x), options = opts)
saveRDS(sinf, file.path(dir, "sin.rds"), version = 2, compress = FALSE)
saveRDS(expm1f, file.path(dir, "expm1.rds"), version = 2, compress = FALSE)
saveRDS(floorf, file.path(dir, "floor.rds"), version = 2, compress = FALSE)
stopifnot(isTRUE(all.equal(sinf(pi / 2), 1)))
stopifnot(isTRUE(all.equal(expm1f(1), exp(1) - 1)))
stopifnot(identical(floorf(c(1.7, NA, -1.2)), c(1, NA, -2)))
stopifnot(identical(floorf(numeric(0)), numeric(0)))

