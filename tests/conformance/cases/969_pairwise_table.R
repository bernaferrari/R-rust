pt <- pairwise.table(function(i, j) 0.04, c("a", "b", "c"), "none")
cat(paste(as.vector(pt), collapse = ","), "\n", sep = "")
