d <- spline(1:5, (1:5)^2, xout = 2.5)
cat(sprintf("%.8f", d$y), "\n", sep = "")
cat(sprintf("%.8f", d$x), "\n", sep = "")
