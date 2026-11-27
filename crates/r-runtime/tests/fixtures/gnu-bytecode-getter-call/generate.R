#!/usr/bin/env Rscript
# Homebrew/pinned GNU compiler emit GETFUN + PUSHNULLARG + GETTER_CALL=99 +
# SWAP=100 for complex assignments: names(x)[1] <- v and attr(x, "a")[1] <- v.
# optimize=3 keeps GETVAR v; STARTASSIGN x; GETFUN; args; GETTER_CALL; SWAP;
# STARTSUBASSIGN_N; LDCONST; VECSUBASSIGN; GETFUN; SETTER_CALL; ENDASSIGN;
# POP; GETVAR x; RETURN.
# Uncompressed XDR version 2 lets tests mutate the opcode without changing
# retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-getter-call"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
names_sub <- cmpfun(function(x, v) { names(x)[1] <- v; x }, options = opts)
attr_sub <- cmpfun(function(x, v) { attr(x, "a")[1] <- v; x }, options = opts)
saveRDS(names_sub, file.path(dir, "names.rds"), version = 2, compress = FALSE)
saveRDS(attr_sub, file.path(dir, "attr.rds"), version = 2, compress = FALSE)
x <- c(a = 1, b = 2, c = 3)
names(x)[1] <- "z"
stopifnot(identical(names_sub(c(a = 1, b = 2, c = 3), "z"), x))
y <- 1:3
attr(y, "a") <- c("p", "q", "r")
attr(y, "a")[1] <- "z"
stopifnot(identical(attr_sub(structure(1:3, a = c("p", "q", "r")), "z"), y))
