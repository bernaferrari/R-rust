d <- HoltWinters(ts(1:20, frequency = 4))
cat(paste(names(d$coefficients), collapse = ","), "\n", sep = "")
cat(sprintf("%.8f", as.vector(d$coefficients["b"])), "\n", sep = "")
