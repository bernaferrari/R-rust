# Homebrew/pinned GNU compiler emit ISNULL=75 .. ISOBJECT=82 with no operands.
# optimize=3 drops BASEGUARD so the stream is GETVAR; IS*; RETURN. Uncompressed
# XDR version 2 lets tests mutate the opcode without changing retained source.
# is.numeric has no ISNUMERIC inline handler in current GNU compiler; omit it.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-istype"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
opts <- list(optimize = 3L)
fixtures <- list(
    null = cmpfun(function(x) is.null(x), options = opts),
    logical = cmpfun(function(x) is.logical(x), options = opts),
    integer = cmpfun(function(x) is.integer(x), options = opts),
    double = cmpfun(function(x) is.double(x), options = opts),
    complex = cmpfun(function(x) is.complex(x), options = opts),
    character = cmpfun(function(x) is.character(x), options = opts),
    symbol = cmpfun(function(x) is.symbol(x), options = opts),
    object = cmpfun(function(x) is.object(x), options = opts)
)
for (name in names(fixtures)) {
    saveRDS(fixtures[[name]], file.path(dir, paste0(name, ".rds")),
            version = 2, compress = FALSE)
}
stopifnot(identical(fixtures$null(NULL), TRUE))
stopifnot(identical(fixtures$integer(factor(1)), FALSE))
stopifnot(identical(fixtures$object(factor(1)), TRUE))
