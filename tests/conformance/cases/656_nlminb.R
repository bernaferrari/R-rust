d <- nlminb(1, function(x) x^2)
cat(abs(d$par) < 1e-6, "\n", sep = "")
cat(abs(d$objective) < 1e-12, "\n", sep = "")
