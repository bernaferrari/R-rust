options(digits = 7)
cat(paste(sprintf("%.8f%+.8fi", Re(zapsmall(polyroot(1:4), digits = 10)), Im(zapsmall(polyroot(1:4), digits = 10))), collapse = " | "), "\n", sep = "")
