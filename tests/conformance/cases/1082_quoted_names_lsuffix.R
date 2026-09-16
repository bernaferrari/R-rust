x <- 1:2
x[c("2", "2")] <- 4
y <- c(1:2, "2" = 4)
cat(paste(encodeString(names(x), quote = "'"), collapse = ","), "\n", sep = "")
cat(paste(encodeString(names(y), quote = "'"), collapse = ","), "\n", sep = "")
cat(isTRUE(all.equal(x, y)), "\n", sep = "")
cat(paste(encodeString(names(as.list(quote(c(1:2, "2" = 4)))), quote = "'"), collapse = ","), "\n", sep = "")
cat(typeof(1e-3L), "\n", sep = "")
