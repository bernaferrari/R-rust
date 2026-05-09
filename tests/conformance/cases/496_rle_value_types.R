z <- rle(c("a", "a", "b", NA, NA, "b"))
cat(paste(z$lengths, collapse = "|"), "\n", sep = "")
cat(paste(ifelse(is.na(z$values), "NA", z$values), collapse = "|"), "\n", sep = "")
cat(typeof(z$values), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")

w <- rle(c(TRUE, TRUE, FALSE, NA, NA, FALSE))
cat(paste(w$lengths, collapse = "|"), "\n", sep = "")
cat(paste(ifelse(is.na(w$values), "NA", w$values), collapse = "|"), "\n", sep = "")
cat(typeof(w$values), "\n", sep = "")
cat(paste(class(w), collapse = "|"), "\n", sep = "")

u <- inverse.rle(list(lengths = c(2L, 1L), values = c("a", "b")))
cat(paste(u, collapse = "|"), "\n", sep = "")
cat(typeof(u), "\n", sep = "")

v <- inverse.rle(list(lengths = c(1L, 2L), values = c(TRUE, NA)))
cat(paste(ifelse(is.na(v), "NA", v), collapse = "|"), "\n", sep = "")
cat(typeof(v), "\n", sep = "")
