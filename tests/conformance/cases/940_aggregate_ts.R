cat(paste(as.vector(aggregate.ts(ts(1:8, frequency = 4), nfrequency = 1, FUN = sum)), collapse = ","), "\n", sep = "")
