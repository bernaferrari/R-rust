f <- splinefun(1:5, (1:5)^2)
cat(sprintf("%.8f", f(2.5)), "\n", sep = "")
cat(sprintf("%.8f", f(1)), "\n", sep = "")
cat(sprintf("%.8f", f(5)), "\n", sep = "")
