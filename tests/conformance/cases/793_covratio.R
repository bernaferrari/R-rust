y <- c(1, 2, 2, 4, 5)
x <- 1:5
fit <- lm(y ~ x)
cr <- covratio(fit)
# Observation 3 leaves an exact linear fit: its covratio divides by a
# degenerate ~0 variance and GNU builds render it as 0 or NaN depending
# on the contracted residual arithmetic. Print the four meaningful ratios
# and classify the degenerate one.
cat(paste(round(cr[-3], 4), collapse = ","), "\n", sep = "")
cat(is.nan(cr[3]) || cr[3] == 0, "\n", sep = "")
