d <- HoltWinters(ts(1:20, frequency = 4))
cat(sprintf("%.8f", d$alpha), "\n", sep = "")
cat(class(d), "\n", sep = "")
