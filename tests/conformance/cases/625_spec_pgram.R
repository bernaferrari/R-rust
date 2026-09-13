x <- sin(2 * pi * (0:15) / 16)
d <- spec.pgram(x, plot = FALSE, detrend = FALSE, taper = 0)
cat(paste(sprintf("%.8f", d$spec), collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
