mod <- makeARIMA(0.5, numeric(0), numeric(0))
mod$h <- 1
ks <- KalmanSmooth(c(1, 2, 1, 2, 1), mod)
cat(paste(round(as.vector(ks$smooth), 4), collapse = ","), "\n", sep = "")
cat(paste(round(as.vector(ks$var), 4), collapse = ","), "\n", sep = "")
