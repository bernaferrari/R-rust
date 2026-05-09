z <- as.factor(c("b", "a", "b"))
cat(paste(as.character(z), collapse = "|"), "\n", sep = "")
cat(paste(levels(z), collapse = "|"), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")

f <- factor(c("b", "a"), levels = c("b", "a", "c"))
g <- as.factor(f)
cat(identical(f, g), "\n", sep = "")

o <- as.ordered(f)
cat(paste(as.character(o), collapse = "|"), "\n", sep = "")
cat(paste(levels(o), collapse = "|"), "\n", sep = "")
cat(paste(class(o), collapse = "|"), "\n", sep = "")

p <- as.ordered(c("b", "a", "b"))
cat(paste(levels(p), collapse = "|"), "\n", sep = "")
cat(paste(class(p), collapse = "|"), "\n", sep = "")
