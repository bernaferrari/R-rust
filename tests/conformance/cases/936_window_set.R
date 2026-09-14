x <- ts(1:8)
window(x, start = 2, end = 4) <- 0
cat(paste(as.vector(x), collapse = ","), "\n", sep = "")
