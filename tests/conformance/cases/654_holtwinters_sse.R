d <- HoltWinters(ts(1:20, frequency = 4))
cat(sprintf("%.8f", d$beta), "\n", sep = "")
cat(sprintf("%.8f", d$SSE), "\n", sep = "")
