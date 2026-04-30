paths <- c(tempfile(), tempfile(), tempfile())
print(length(unique(paths)))
print(all(!file.exists(paths)))

