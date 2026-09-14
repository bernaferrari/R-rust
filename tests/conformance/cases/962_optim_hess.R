f <- function(p) (p[1] - 2)^2 + (p[2] + 1)^2
cat(paste(round(as.vector(optimHess(c(0, 0), f)), 4), collapse = ","), "\n", sep = "")
