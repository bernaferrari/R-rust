fit <- list(residuals = 1:10, df.residual = 7, aic = 20)
class(fit) <- "glm"
cat(paste(extractAIC(fit), collapse = ","), "\n", sep = "")
cat(paste(extractAIC(fit, k = 4), collapse = ","), "\n", sep = "")
