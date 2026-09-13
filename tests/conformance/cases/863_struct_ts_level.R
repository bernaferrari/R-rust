s <- StructTS(ts(c(1, 2, 1, 2, 1, 2, 1, 2, 1, 2)), type = "level")
cat(paste(round(as.numeric(s$coef), 4), collapse = ","), "\n", sep = "")
s2 <- StructTS(ts(1:10), type = "level")
cat(paste(round(as.numeric(s2$coef), 4), collapse = ","), "\n", sep = "")
