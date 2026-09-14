p <- ppr(1:5, c(1.1, 1.9, 3.2, 3.8, 5.1), nterms = 1)
cat(paste(round(as.vector(p$fitted.values), 4), collapse = ","), "\n", sep = "")
