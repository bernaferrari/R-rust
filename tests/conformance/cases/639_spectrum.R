d <- spectrum(1:16, plot = FALSE, detrend = FALSE, taper = 0)
cat(sprintf("%.8f", d$spec[1]), "\n", sep = "")
cat(class(d), "\n", sep = "")
