f <- ecdf(1:3)
cat(paste(sprintf("%.10f", f(0:4)), collapse = ","), "\n", sep = "")
cat(paste(class(f), collapse = ","), "\n", sep = "")
cat(is.function(f), "\n", sep = "")
