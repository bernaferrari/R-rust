x <- 1:20
y <- sin(x / 3)
k <- ksmooth(x, y, kernel = "normal", bandwidth = 2)
cat(sprintf("%.4f", k$y[5]), "\n", sep = "")
cat(sprintf("%.4f", k$y[10]), "\n", sep = "")
cat(length(k$y), "\n", sep = "")
cat(names(k)[1], "\n", sep = "")
