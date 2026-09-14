f <- stepfun(c(1, 2, 4), c(10, 20, 30, 40))
cat(paste(knots(f), collapse = ","), "\n", sep = "")
cat(paste(f(c(0, 1, 1.5, 2, 3, 4, 5)), collapse = ","), "\n", sep = "")
