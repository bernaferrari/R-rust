z <- interaction(c("b", "a", "b"), c("x", "x", "y"))
cat(paste(as.character(z), collapse = "|"), "\n", sep = "")
cat(paste(levels(z), collapse = "|"), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")
