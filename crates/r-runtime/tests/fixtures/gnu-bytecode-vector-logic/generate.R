# GNU R oracle bac583951b728e97b9786804d3b4081f0fe18df5.
# AND/OR/NOT are opcodes 57-59. Uncompressed XDR version 2 lets tests mutate
# the instruction stream without changing retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-vector-logic"
dir.create(dir, showWarnings = FALSE, recursive = TRUE)
fixtures <- list(
    and = cmpfun(function(x, y) x & y),
    or = cmpfun(function(x, y) x | y),
    not = cmpfun(function(x) !x)
)
for (name in names(fixtures)) {
    saveRDS(fixtures[[name]], file.path(dir, paste0(name, ".rds")),
            version = 2, compress = FALSE)
}
stopifnot(identical(fixtures$and(c(TRUE, FALSE, NA), TRUE), c(TRUE, FALSE, NA)))
stopifnot(identical(fixtures$or(c(FALSE, TRUE, NA), FALSE), c(FALSE, TRUE, NA)))
stopifnot(identical(fixtures$not(c(TRUE, FALSE, NA)), c(FALSE, TRUE, NA)))
