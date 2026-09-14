f <- tempfile()
x <- ftable(array(1:4, dim = c(2, 2)))
invisible(write.ftable(x, file = f))
cat(paste(as.vector(read.ftable(f)), collapse = ","), "\n", sep = "")
