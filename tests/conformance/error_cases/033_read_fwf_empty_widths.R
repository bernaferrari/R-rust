f <- tempfile()
writeLines(c("123", "456"), f)
read.fwf(f, widths = integer(0))
