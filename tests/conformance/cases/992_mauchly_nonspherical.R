S <- matrix(c(4, 1, 0, 1, 3, 1, 0, 1, 2), 3, 3)
obj <- list(SSD = S, df = 10)
class(obj) <- "SSD"
m <- mauchly.test(obj)
cat(paste(c(round(as.vector(m$statistic), 6), round(m$p.value, 6)), collapse = ","), "\n", sep = "")
