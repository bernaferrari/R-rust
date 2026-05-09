z <- aggregate(c(1, NA, 3), list(g = c("a", "a", "b")), length)
cat(paste(z[[1]], collapse = "|"), "\n", sep = "")
cat(paste(z[[2]], collapse = "|"), "\n", sep = "")
