fit <- aov(c(1.1, 1.9, 3.2, 3.8, 5.1) ~ I(1:5))
cat(paste(round(summary.aov(fit)[[1]][, "F value"], 4), collapse = ","), "\n", sep = "")
