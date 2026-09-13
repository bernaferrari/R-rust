cat(is.tskernel(kernel("daniell", 1)), "\n", sep = "")
d <- spectrum(1:16, method = "ar", plot = FALSE)
cat(sprintf("%.8f", d$spec[1]), "\n", sep = "")
cat(class(d), "\n", sep = "")
