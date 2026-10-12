y <- c(1, 2, 2, 4, 5)
x <- 1:5
inf <- lm.influence(lm(y ~ x), do.coef = FALSE)
cat(paste(round(inf$hat, 4), collapse = ","), "\n", sep = "")
s <- inf$sigma
# Observation 3 leaves an exact linear fit (y = x on the remaining four
# points): its leave-one-out sigma is a degenerate sqrt-of-~0 that GNU
# builds render as 0 or NaN depending on the FP contraction of the QR
# residual dot product. Print the four meaningful sigmas and classify the
# degenerate one instead of equating unlike values.
cat(paste(round(s[-3], 4), collapse = ","), "\n", sep = "")
cat(is.nan(s[3]) || s[3] == 0, "\n", sep = "")
cat(paste(round(inf$wt.res, 4), collapse = ","), "\n", sep = "")
