d <- dist(1:4)
h <- hclust(d)
cat(paste(round(as.vector(cophenetic(h)), 4), collapse = ","), "\n", sep = "")
cat(class(cophenetic(h)), "\n", sep = "")
