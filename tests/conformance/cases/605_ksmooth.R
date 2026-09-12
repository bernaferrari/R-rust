d <- ksmooth(1:5, c(1, 2, 1, 2, 1), bandwidth = 2, x.points = 1:5)
cat(paste(d$x, collapse = ","), "\n", sep = "")
cat(paste(sprintf("%.8f", d$y), collapse = ","), "\n", sep = "")
