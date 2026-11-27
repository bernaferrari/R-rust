# Run with the pinned GNU R oracle: bac583951b728e97b9786804d3b4081f0fe18df5.
# Invocation from repository root.
f <- compiler::cmpfun(function(x, y=2) { z <- x+y; if(z>0) z*2 else -z })
saveRDS(f, 'crates/r-embed/tests/fixtures/gnu-compiled-closure.rds', version=2, compress=FALSE)
g <- local({
    offset <- 3
    compiler::cmpfun(function(x=2) {
        f <- function(z) z + offset
        total <- 0
        for (i in seq_len(x)) total <- total + f(i)
        total
    })
})
saveRDS(g, 'crates/r-embed/tests/fixtures/gnu-compiled-captured.rds', version=3, compress=FALSE)
constant <- compiler::cmpfun(function() 42L)
saveRDS(constant, 'crates/r-embed/tests/fixtures/gnu-constant-closure.rds', version=2, compress=FALSE)
null_value <- compiler::cmpfun(function() NULL)
saveRDS(null_value, 'crates/r-embed/tests/fixtures/gnu-null-closure.rds', version=2, compress=FALSE)
true_value <- compiler::cmpfun(function() TRUE)
saveRDS(true_value, 'crates/r-embed/tests/fixtures/gnu-true-closure.rds', version=2, compress=FALSE)
false_value <- compiler::cmpfun(function() FALSE)
saveRDS(false_value, 'crates/r-embed/tests/fixtures/gnu-false-closure.rds', version=2, compress=FALSE)
cat(f(3), f(-5), g(), g(4), constant(), '\n')
