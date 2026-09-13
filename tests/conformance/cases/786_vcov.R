vcov.list <- function(object, ...) object$vcov
obj <- list(vcov = diag(c(0.25, 1)))
v <- vcov(obj)
cat(paste(as.vector(v), collapse = ","), "\n", sep = "")
cat(paste(dim(v), collapse = ","), "\n", sep = "")
