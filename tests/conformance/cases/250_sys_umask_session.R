old <- Sys.umask("077")
print(inherits(old, "octmode"))
print(is.integer(old))

f <- tempfile()
print(file.create(f))
print(as.character(file.info(f)$mode))

d <- tempfile()
print(dir.create(d))
print(as.character(file.info(d)$mode))

prev <- Sys.umask("022")
print(as.character(prev))
print(as.character(Sys.umask()))

unlink(c(f, d), recursive = TRUE)
invisible(Sys.umask(old))
