d <- nlm(function(x) x^2, 1)
cat(abs(d$estimate) < 1e-6, "\n", sep = "")
cat(abs(d$minimum) < 1e-12, "\n", sep = "")
