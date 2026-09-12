d <- smooth.spline(1:8, 1:8)
cat(paste(d$y, collapse = ","), "\n", sep = "")
cat(d$n, "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
