d <- as.vector(filter(1:5, rep(1 / 3, 3)))
cat(paste(ifelse(is.na(d), "NA", sprintf("%.8f", d)), collapse = ","), "\n", sep = "")
