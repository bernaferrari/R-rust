cat(iconv("a", to = "ASCII"), "\n", sep = "")
cat(iconv("abc", to = "UTF-8"), "\n", sep = "")
cat(paste(iconv(c("a", "b"), to = "ASCII"), collapse = ","), "\n", sep = "")
cat(iconv("a", from = "UTF-8", to = "ASCII"), "\n", sep = "")
