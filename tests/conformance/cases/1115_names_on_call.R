e2 <- quote(c(a = 1, b = 2))
cat(paste(names(e2), collapse = "|"), "\n", sep = "")
names(e2)[2] <- "a b c"
cat(paste(names(e2), collapse = "|"), "\n", sep = "")
cat(deparse(e2, control = "all"), "\n", sep = "")
cat(length(as.list(invisible)), "\n")
