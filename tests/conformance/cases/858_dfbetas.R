x <- 1:5
y <- c(1, 2, 2, 4, 5)
d <- dfbetas(lm(y ~ x))
# Observation 3 leaves an exact linear fit: its standardized dfbetas
# divide by a degenerate ~0 leave-one-out scale, so GNU builds produce
# NaN, exact 0, or an astronomically large finite value for that row
# depending on the contracted QR arithmetic. Print the meaningful
# observations' influence and classify the degenerate row.
cat(paste(round(as.vector(d[-3, ]), 4), collapse = ","), "\n", sep = "")
cat(all(is.nan(d[3, ]) | d[3, ] == 0 | abs(d[3, ]) > 1e6), "\n", sep = "")
