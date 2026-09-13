set.seed(1)
x <- arima.sim(list(ar = 0.5), n = 6, n.start = 1)
cat(paste(round(as.numeric(x), 4), collapse = ","), "\n", sep = "")
