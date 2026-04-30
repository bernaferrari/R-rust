f <- tempfile()
writeLines("1 + 2", f)
print(dget(f))
