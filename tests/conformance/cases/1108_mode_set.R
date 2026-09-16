x <- 1:3
names(x) <- c("a", "b", "c")
mode(x) <- "double"
cat(typeof(x), "\n", sep = "")
cat(paste(names(x), collapse = ","), "\n", sep = "")
cat(paste(x, collapse = ","), "\n", sep = "")
