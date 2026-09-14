y <- ts(c(1, 2, 1, 2, 1, 2, 1, 2, 1, 2))
s <- tsSmooth(StructTS(y, type = "level"))
cat(paste(round(as.numeric(s), 4), collapse = ","), "\n", sep = "")
