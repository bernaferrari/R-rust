# The fixture runner puts a valid existing PDF at this destination.
before <- dev.cur()
pdf("protected.pdf")
plot(1:3, 1:3)
rasterImage(matrix("navy", 2L, 2L), 1, 1, 2, 2)
failure <- tryCatch({ dev.off(); "" }, error = conditionMessage)
stopifnot(grepl("PDF raster images are not supported", failure, fixed = TRUE))
stopifnot(identical(dev.cur(), before))
cat("Retained the previous file after refusing unsupported PDF content\n")
