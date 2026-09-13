d <- ar(1:20, aic = FALSE, order.max = 1)
cat(sprintf("%.8f", as.vector(d$ar)), "\n", sep = "")
cat(d$order, "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
