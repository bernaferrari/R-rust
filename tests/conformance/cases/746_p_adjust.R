cat(paste(round(p.adjust(c(0.01, 0.04, 0.2)), 7), collapse = ","), "\n", sep = "")
cat(paste(round(p.adjust(c(0.01, 0.04, 0.2), "bonferroni"), 7), collapse = ","), "\n", sep = "")
cat(paste(round(p.adjust(c(0.01, 0.04, 0.2), "BH"), 7), collapse = ","), "\n", sep = "")
cat(paste(round(p.adjust(c(0.01, 0.04, 0.2), "none"), 7), collapse = ","), "\n", sep = "")
