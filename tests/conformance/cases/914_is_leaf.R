x <- 1
attr(x, "leaf") <- TRUE
cat(is.leaf(x), "\n", sep = "")
cat(is.leaf(1), "\n", sep = "")
