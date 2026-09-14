d <- data.frame(x = c(1, 1, 2, 2), y = c(10, 20, 30, 40))
a <- aggregate.data.frame(d["y"], d["x"], sum)
cat(paste(a$y, collapse = ","), "\n", sep = "")
