r <- serialize(list(a = 1:2, b = "x"), NULL, ascii = TRUE)
cat(length(r) > 0, "\n", sep = "")
cat(any(as.integer(r) < 9 | as.integer(r) > 126), "\n", sep = "")
cat(identical(unserialize(r), list(a = 1:2, b = "x")), "\n", sep = "")
