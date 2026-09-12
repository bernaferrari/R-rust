d <- kmeans(1:6, 2)
cat(paste(sort(d$size), collapse = ","), "\n", sep = "")
cat(paste(sort(round(as.vector(d$centers), 8)), collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
