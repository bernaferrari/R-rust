cooks.distance.list <- function(model, ...) {
  h <- model$hat
  ((model$residuals / ((1 - h) * model$sigma))^2 * h) / model$rank
}
obj <- list(residuals = c(1, -1, 0.5, -0.5), hat = c(0.7, 0.3, 0.3, 0.7), sigma = 1, rank = 2)
cd <- cooks.distance(obj)
cat(paste(round(cd, 4), collapse = ","), "\n", sep = "")
