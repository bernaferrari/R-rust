mod <- makeARIMA(0.5, numeric(0), numeric(0))
kf <- KalmanForecast(3, mod)
cat(paste(round(kf$pred, 4), collapse = ","), "\n", sep = "")
cat(paste(round(kf$var, 4), collapse = ","), "\n", sep = "")
