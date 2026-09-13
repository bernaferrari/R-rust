cat("UTF-8" %in% iconvlist(), "\n", sep = "")
cat(any(iconvlist() %in% c("ASCII", "US-ASCII")), "\n", sep = "")
cat(is.character(iconvlist()), "\n", sep = "")
cat(identical(iconvlist(), sort(iconvlist())), "\n", sep = "")
