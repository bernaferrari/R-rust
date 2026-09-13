m <- matrix(c(0, 1, 2, 1, 0, 3, 2, 3, 0), 3, 3)
rownames(m) <- c("a", "b", "c")
cat(paste(labels(as.dist(m)), collapse = ","), "\n", sep = "")
