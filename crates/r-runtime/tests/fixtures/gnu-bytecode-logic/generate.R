# GNU R oracle bac583951b728e97b9786804d3b4081f0fe18df5.
# AND1ST/AND2ND/OR1ST/OR2ND are opcodes 88-91. Uncompressed XDR version 2
# lets tests mutate the instruction stream without changing retained source.
library(compiler)
dir <- "crates/r-embed/tests/fixtures/gnu-bytecode-logic"
fixtures <- list(
    and = cmpfun(function(x, y) x && y),
    or = cmpfun(function(x, y) x || y)
)
for (name in names(fixtures)) {
    saveRDS(fixtures[[name]], file.path(dir, paste0(name, ".rds")),
            version = 2, compress = FALSE)
}
stopifnot(identical(fixtures$and(TRUE, FALSE), FALSE))
stopifnot(identical(fixtures$or(FALSE, TRUE), TRUE))
