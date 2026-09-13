d <- suppressWarnings(optim(1, function(x) x^2))
cat(abs(d$par) < 1e-4, "\n", sep = "")
cat(abs(d$value) < 1e-8, "\n", sep = "")
