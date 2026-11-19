passengers <- log(AirPassengers)
train <- window(passengers, end = c(1959, 12))
held_out <- window(passengers, start = c(1960, 1))
fit <- arima(train, order = c(0, 1, 1),
             seasonal = list(order = c(0, 1, 1), period = 12),
             method = "CSS")
forecast <- predict(fit, n.ahead = length(held_out))
months <- c(1, 6, 12)
rmse <- sqrt(mean((forecast$pred - held_out)^2))

stopifnot(length(train) == 132L, length(held_out) == 12L,
          isTRUE(all.equal(tsp(forecast$pred), tsp(held_out), tolerance = 1e-12)),
          isTRUE(all.equal(tsp(forecast$se), tsp(held_out), tolerance = 1e-12)),
          isTRUE(all.equal(as.numeric(exp(forecast$pred[months])),
                           c(419.4238817, 547.1379448, 452.3636174), tolerance = 1e-5)),
          isTRUE(all.equal(as.numeric(forecast$se[months]),
                           c(0.03680916, 0.06653184, 0.09006870), tolerance = 1e-5)),
          isTRUE(all.equal(rmse, 0.04029175, tolerance = 1e-5)))
cat("Seasonal forecasts and held-out error agree\n")
