Y <- cbind(c(1, 2, 3, 4, 5, 6, 7, 8), c(1.2, 2.1, 2.8, 4.2, 4.9, 6.3, 6.8, 8.1))
g <- factor(rep(1:2, each = 4))
s <- summary.manova(manova(Y ~ g))
cat(paste(round(as.vector(s$stats[1, ]), 4), collapse = ","), "\n", sep = "")
