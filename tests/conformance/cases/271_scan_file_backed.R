f <- tempfile()
writeLines(c("1 2", "3 NA"), f)
num <- scan(f, numeric(), -1, quiet = TRUE)

g <- tempfile()
writeLines("alpha beta NA", g)
chr <- scan(g, "", -1, quiet = TRUE)

cat(paste(num, collapse = ","), typeof(num), length(num), "\n")
cat(paste(chr, collapse = ","), typeof(chr), length(chr))
