hc <- hclust(dist(1:5), method = "complete")
cat(paste(cutree(hc, k = 2), collapse = ","), "\n", sep = "")
cat(paste(cutree(hc, k = 3), collapse = ","), "\n", sep = "")
cat(paste(cutree(hc, h = 1.5), collapse = ","), "\n", sep = "")
