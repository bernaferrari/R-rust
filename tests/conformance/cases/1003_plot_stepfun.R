f <- ecdf(c(1, 2, 2, 4))
s <- plot.stepfun(f)
cat(paste(round(s$t, 4), collapse = ","), "\n", sep = "")
cat(paste(round(s$y, 4), collapse = ","), "\n", sep = "")
