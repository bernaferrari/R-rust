x <- arima.sim(list(ar = 0.5), n = 4, n.start = 1, start.innov = 1, innov = c(1, 1, 1, 1))
cat(paste(round(as.numeric(x), 4), collapse = ","), "\n", sep = "")
