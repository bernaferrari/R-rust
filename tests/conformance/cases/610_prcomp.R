d <- prcomp(matrix(1:12, 4, 3), center = TRUE, scale. = FALSE)
cat(paste(sprintf("%.8f", d$sdev), collapse = ","), "\n", sep = "")
cat(paste(dim(d$rotation), collapse = "x"), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
