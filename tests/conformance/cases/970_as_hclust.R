hc <- hclust(dist(1:4))
cat(paste(round(as.hclust(hc)$height, 4), collapse = ","), "\n", sep = "")
