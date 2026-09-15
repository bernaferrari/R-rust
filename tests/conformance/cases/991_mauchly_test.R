obj <- list(SSD = 4 * diag(3), df = 4)
class(obj) <- "SSD"
cat(paste(c(as.vector(mauchly.test(obj)$statistic), mauchly.test(obj)$p.value), collapse = ","), "\n", sep = "")
