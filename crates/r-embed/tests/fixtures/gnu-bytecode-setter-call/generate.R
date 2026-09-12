#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit GETFUN + PUSHNULLARG + SETTER_CALL=98
# for replacement functions: names(x) <- v and attr(x, "a") <- v.
# optimize=3 keeps GETVAR v; STARTASSIGN x; GETFUN; args; SETTER_CALL;
# ENDASSIGN; POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-setter-call"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
names_set <- cmpfun(function(x, v) { names(x) <- v; x }, options = opts)
attr_set <- cmpfun(function(x, v) { attr(x, "a") <- v; x }, options = opts)
saveRDS(names_set, file.path(dir, "names.rds"), version = 2, compress = FALSE)
saveRDS(attr_set, file.path(dir, "attr.rds"), version = 2, compress = FALSE)
x <- 1:3
names(x) <- c("a", "b", "c")
stopifnot(identical(names_set(1:3, c("a", "b", "c")), x))
y <- 1:2
attr(y, "a") <- 9L
stopifnot(identical(attr_set(1:2, 9L), y))
