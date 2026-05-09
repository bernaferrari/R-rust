f <- factor(c("a", NA, "b"), levels = c("a", "b"))
z <- addNA(f)
cat(paste(as.character(z), collapse = "|"), "\n", sep = "")
cat(paste(ifelse(is.na(levels(z)), "NA", levels(z)), collapse = "|"), "\n", sep = "")
cat(paste(is.na(z), collapse = "|"), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")

g <- addNA(factor(c("a", "b")), ifany = TRUE)
cat(paste(ifelse(is.na(levels(g)), "NA", levels(g)), collapse = "|"), "\n", sep = "")

h <- addNA(c("b", NA, "a"))
cat(paste(as.character(h), collapse = "|"), "\n", sep = "")
cat(paste(ifelse(is.na(levels(h)), "NA", levels(h)), collapse = "|"), "\n", sep = "")
