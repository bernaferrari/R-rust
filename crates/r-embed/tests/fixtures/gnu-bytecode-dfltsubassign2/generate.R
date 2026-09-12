#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBASSIGN2=71, DOMISSING=30,
# DFLTSUBASSIGN2=72 for missing-index [[ replacement: x[[]] <- v.
# Present indices stay on STARTSUBASSIGN2_N / VECSUBASSIGN2.
# optimize=3 keeps GETVAR v; STARTASSIGN x; STARTSUBASSIGN2; DOMISSING;
# DFLTSUBASSIGN2; ENDASSIGN; POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dfltsubassign2"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
empty2 <- cmpfun(function(x, v) { x[[]] <- v; x }, options = opts)
saveRDS(empty2, file.path(dir, "empty.rds"), version = 2, compress = FALSE)
d <- structure(list(1L, 2L), class = "foo")
"[[<-.foo" <- function(x, i, value) "assigned"
stopifnot(identical(empty2(d, 9L), "assigned"))
