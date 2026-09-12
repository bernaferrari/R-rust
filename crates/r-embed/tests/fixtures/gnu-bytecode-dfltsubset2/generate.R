#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit STARTSUBSET2=69, DOMISSING=30,
# DFLTSUBSET2=70 for missing-index [[ extraction: x[[]].
# Present indices stay on STARTSUBSET2_N / VECSUBSET2.
# optimize=3 keeps GETVAR x; STARTSUBSET2; DOMISSING; DFLTSUBSET2; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-dfltsubset2"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
empty2 <- cmpfun(function(x) x[[]], options = opts)
saveRDS(empty2, file.path(dir, "empty.rds"), version = 2, compress = FALSE)
d <- structure(list(1L, 2L), class = "foo")
"[[.foo" <- function(x, i) "got"
stopifnot(identical(empty2(d), "got"))
tryCatch(empty2(list(1L, 2L)), error = function(e) {
  stopifnot(grepl("missing subscript", conditionMessage(e), fixed = TRUE))
})
