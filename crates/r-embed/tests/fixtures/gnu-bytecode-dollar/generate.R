# Homebrew/pinned GNU compiler emit DOLLAR=73 and DOLLARGETS=74.
# optimize=3 drops BASEGUARD so the get stream is GETVAR; DOLLAR; RETURN.
# Assignment is GETVAR; STARTASSIGN; DOLLARGETS; ENDASSIGN; POP; GETVAR; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dollar"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
dollar <- cmpfun(function(x) x$a, options = opts)
dollargets <- cmpfun(function(x, v) { x$a <- v; x }, options = opts)
saveRDS(dollar, file.path(dir, "dollar.rds"), version = 2, compress = FALSE)
saveRDS(dollargets, file.path(dir, "dollargets.rds"), version = 2, compress = FALSE)
stopifnot(identical(dollar(list(a = 1L, b = 2L)), 1L))
stopifnot(identical(dollargets(list(b = 2L), 9L)$a, 9L))
