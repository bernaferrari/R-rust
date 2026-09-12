d <- stl(ts(1:36, frequency = 12), s.window = 7)
cat(paste(dim(d$time.series), collapse = "x"), "\n", sep = "")
gnu <- c(1.355036, 12.017471, 24.001069, 35.644027)
got <- as.vector(d$time.series[, "trend"])[c(1, 12, 24, 36)]
cat(max(abs(got - gnu)) < 0.01, "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
