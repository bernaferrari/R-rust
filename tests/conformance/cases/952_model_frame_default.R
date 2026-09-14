d <- data.frame(y = 1:4, x = c(1, 2, 3, 4))
mf <- model.frame.default(y ~ x, d)
cat(paste(mf$y, collapse = ","), "\n", sep = "")
cat(paste(mf$x, collapse = ","), "\n", sep = "")
