d <- line(1:5, c(1, 2, 1, 2, 3))
cat(paste(sprintf("%.10f", d$coefficients), collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
