dffits.list <- function(model, ...) {
  model$residuals * sqrt(model$hat) / (model$sigma * (1 - model$hat))
}
obj <- list(residuals = c(1, -1, 0.5, -0.5), hat = c(0.7, 0.3, 0.3, 0.7), sigma = 1)
df <- dffits(obj)
cat(paste(round(df, 4), collapse = ","), "\n", sep = "")
