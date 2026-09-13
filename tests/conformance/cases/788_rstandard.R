rstandard.list <- function(model, ...) {
  model$residuals / (model$sigma * sqrt(1 - model$hat))
}
obj <- list(residuals = c(1, -1, 0.5, -0.5), hat = c(0.7, 0.3, 0.3, 0.7), sigma = 1)
rs <- rstandard(obj)
cat(paste(round(rs, 4), collapse = ","), "\n", sep = "")
