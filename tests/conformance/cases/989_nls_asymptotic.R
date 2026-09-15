xy <- sortedXyData(1:5, c(1, 2, 3, 3.5, 3.8))
cat(paste(round(NLSstAsymptotic(xy), 4), collapse = ","), "\n", sep = "")
