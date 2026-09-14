d <- nlm(function(x) (x - 3)^2 + 1, 0)
cat(round(d$estimate, 6), "\n", sep = "")
cat(round(d$minimum, 6), "\n", sep = "")
