hc <- hclust(dist(1:4))
cat(paste(order.dendrogram(as.dendrogram(hc)), collapse = ","), "\n", sep = "")
