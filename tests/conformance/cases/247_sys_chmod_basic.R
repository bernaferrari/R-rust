f <- tempfile()
cat("x", file = f)
missing <- paste0(f, "-missing")

print(Sys.chmod(f, mode = "0600", use_umask = FALSE))
print(as.character(file.info(f)$mode))
print(Sys.chmod(c(f, missing), mode = "0644", use_umask = FALSE))
print(as.character(file.info(f)$mode))
