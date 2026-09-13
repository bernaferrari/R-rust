g <- get_all_vars(y ~ x, list(y = 1:3, x = 4:6, z = 7:9))
cat(paste(g$y, collapse = ","), "\n", sep = "")
cat(paste(g$x, collapse = ","), "\n", sep = "")
cat(paste(names(g), collapse = ","), "\n", sep = "")
