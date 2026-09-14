x <- c(1, 2, 3, 2, 1, 2, 3, 2, 1, 2)
cat(round(as.vector(ar.ols(x, aic = FALSE, order.max = 1, intercept = FALSE)$ar), 4), "\n", sep = "")
