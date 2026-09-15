a <- arima(1:20, order = c(1, 0, 0), method = "ML")
cat(sprintf("%.6f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.1f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(class(a), "\n", sep = "")
