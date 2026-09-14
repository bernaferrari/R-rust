obj <- list(SSD = 4, df = 2)
class(obj) <- "SSD"
cat(as.vector(estVar(obj)), "\n", sep = "")
