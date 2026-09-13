s <- spec.pgram(c(1, 2, 3, 2, 1, 2, 3, 2, 1), plot = FALSE)
cat(paste(round(s$spec, 4), collapse = ","), "\n", sep = "")
cat(paste(round(s$freq, 4), collapse = ","), "\n", sep = "")
