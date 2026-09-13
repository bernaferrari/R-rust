rstudent.list <- function(model, ...) {
  model$residuals / (model$sigma * sqrt(1 - model$hat))
}
obj <- list(
  residuals = c(1, -1, 0.5, -0.5),
  hat = c(0.7, 0.3, 0.3, 0.7),
  sigma = c(1.1, 0.9, 1.0, 1.2)
)
rs <- rstudent(obj)
cat(paste(round(rs, 4), collapse = ","), "\n", sep = "")
