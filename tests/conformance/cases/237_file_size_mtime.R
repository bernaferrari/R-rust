f <- tempfile()
cat("abcd", file = f)
print(file.size(f))
print(length(file.mtime(f)))
print(class(file.mtime(f)))
print(is.na(file.size(paste0(f, "-missing"))))

