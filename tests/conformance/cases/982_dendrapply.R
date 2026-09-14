d <- as.dendrogram(hclust(dist(1:4)))
cat(paste(unlist(dendrapply(d, function(x) x)), collapse = ","), "\n", sep = "")
