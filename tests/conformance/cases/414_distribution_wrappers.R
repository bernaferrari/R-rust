values <- c(qexp(0.5), qpois(0.5, 2), qbinom(0.5, 10, 0.25),
            dunif(0.5), punif(0.5), qunif(0.5))
print(abs(values - c(0.6931471805599453, 2, 2, 1, 0.5, 0.5)) < 1e-6)

tail_log_values <- c(qexp(log(0.5), log.p = TRUE),
                     qpois(0.5, 2, lower.tail = FALSE),
                     qbinom(0.5, 10, 0.25, lower.tail = FALSE),
                     dunif(0.5, log = TRUE),
                     punif(0.5, log.p = TRUE),
                     qunif(0.5, lower.tail = FALSE))
print(abs(tail_log_values - c(0.6931471805599453, 2, 2, 0, -0.6931471805599453, 0.5)) < 1e-6)
