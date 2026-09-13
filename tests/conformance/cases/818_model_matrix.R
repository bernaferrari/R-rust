x <- 1:5
m <- model.matrix(~x)
cat(paste(as.vector(m), collapse = ","), "\n", sep = "")
cat(paste(attr(m, "assign"), collapse = ","), "\n", sep = "")
cat(paste(colnames(m), collapse = ","), "\n", sep = "")
