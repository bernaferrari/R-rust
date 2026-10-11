d <- smooth.spline(1:8, 1:8)
cat(paste(round(d$y, 8), collapse = ","), "\n", sep = "")
cat(d$n, "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
