d <- isoreg(c(1, 2, 1, 3))
cat(paste(d$yf, collapse = ","), "\n", sep = "")
cat(paste(d$yc, collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
