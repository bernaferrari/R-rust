z <- cut(c(0, 0.5, 1, 1.5, 2, NA, 3), breaks = c(0, 1, 2), include.lowest = TRUE)
cat(paste(as.character(z), collapse = "|"), "\n", sep = "")
cat(paste(levels(z), collapse = "|"), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")

w <- cut(c(0, 1, 2), breaks = c(0, 1, 2), right = FALSE, include.lowest = TRUE)
cat(paste(as.character(w), collapse = "|"), "\n", sep = "")
cat(paste(levels(w), collapse = "|"), "\n", sep = "")

y <- cut(c(0.5, 1.5), breaks = c(0, 1, 2), labels = c("lo", "hi"))
cat(paste(as.character(y), collapse = "|"), "\n", sep = "")
cat(paste(levels(y), collapse = "|"), "\n", sep = "")

codes <- cut(c(0.5, 1.5, 3), breaks = c(0, 1, 2), labels = FALSE)
cat(paste(codes, collapse = "|"), "\n", sep = "")
cat(paste(class(codes), collapse = "|"), "\n", sep = "")
