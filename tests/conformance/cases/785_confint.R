vcov.list <- function(object, ...) object$vcov
obj <- list(coefficients = c(a = 1, b = 2), vcov = diag(c(0.25, 1)))
ci <- confint(obj)
cat(paste(round(as.vector(ci), 4), collapse = ","), "\n", sep = "")
cat(paste(rownames(ci), collapse = ","), "\n", sep = "")
cat(paste(colnames(ci), collapse = ","), "\n", sep = "")
