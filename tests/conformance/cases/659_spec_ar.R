d <- spec.ar(1:16, plot = FALSE)
cat(sprintf("%.8f", d$spec[1]), "\n", sep = "")
cat(class(d), "\n", sep = "")
