attach(list(x = 1L))
cat(get("x", envir = as.environment(2)), "\n", sep = "")
cat("x" %in% ls(as.environment(2)), "\n", sep = "")
