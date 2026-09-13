mod <- makeARIMA(0.5, numeric(0), numeric(0))
k <- KalmanLike(c(1, 2, 1, 2, 1), mod)
cat(round(k$Lik, 8), "\n", sep = "")
cat(round(k$s2, 4), "\n", sep = "")
