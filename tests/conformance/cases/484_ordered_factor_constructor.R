z <- ordered(c("b", "a", "b"))
cat(paste(as.character(z), collapse = "|"), "\n", sep = "")
cat(paste(levels(z), collapse = "|"), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")
cat(paste(c(is.ordered(z), is.factor(z)), collapse = "|"), "\n", sep = "")

w <- ordered(c("low", "high"), levels = c("low", "medium", "high"))
cat(paste(as.integer(w), collapse = "|"), "\n", sep = "")
cat(paste(levels(w), collapse = "|"), "\n", sep = "")
