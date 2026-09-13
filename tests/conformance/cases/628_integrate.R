d <- integrate(function(x) x, 0, 1)
cat(sprintf("%.10f", d$value), "\n", sep = "")
cat(class(d), "\n", sep = "")
