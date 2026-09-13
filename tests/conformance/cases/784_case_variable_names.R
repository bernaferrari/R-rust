m <- matrix(1:6, 2, 3, dimnames = list(c("r1", "r2"), c("c1", "c2", "c3")))
cat(paste(case.names(m), collapse = ","), "\n", sep = "")
cat(paste(variable.names(m), collapse = ","), "\n", sep = "")
