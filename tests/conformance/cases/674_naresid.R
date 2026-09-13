cat(paste(naresid(NULL, 1:3), collapse = ","), "\n", sep = "")
cat(inherits(attr(na.omit(c(1, NA, 3)), "na.action"), "omit"), "\n", sep = "")
