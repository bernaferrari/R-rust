m <- matrix(c(0, 0, 10, 10, 10, 0, 0, 1, 10), 3, byrow = TRUE)
h <- heatmap(m)
cat(paste(h$rowInd, collapse = ","), "\n", sep = "")
cat(paste(h$colInd, collapse = ","), "\n", sep = "")
