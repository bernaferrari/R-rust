cat(paste(ifelse(c(TRUE, FALSE, NA), c("yes", "unused", "missing"), c("unused", "no", "unused")), collapse = "|"), "\n", sep = "")
cat(typeof(ifelse(c(TRUE, FALSE), c("x", "y"), c("a", "b"))), "\n")
cat(paste(ifelse(c(TRUE, FALSE, NA), c(1L, 2L, 3L), c(4L, 5L, 6L)), collapse = "|"), "\n", sep = "")
cat(typeof(ifelse(c(TRUE, FALSE), c(TRUE, FALSE), c(FALSE, TRUE))), "\n")
