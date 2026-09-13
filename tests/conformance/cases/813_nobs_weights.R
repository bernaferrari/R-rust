obj <- list(residuals = 1:5, weights = c(1, 0, 1, 1, 0))
class(obj) <- "lm"
cat(nobs(obj), "\n", sep = "")
obj2 <- list(residuals = 1:4)
class(obj2) <- "lm"
cat(nobs(obj2), "\n", sep = "")
