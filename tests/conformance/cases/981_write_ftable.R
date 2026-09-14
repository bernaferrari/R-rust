x <- ftable(array(1:4, dim = c(2, 2)))
invisible(capture.output(y <- write.ftable(x)))
cat(paste(as.vector(y), collapse = ","), "\n", sep = "")
