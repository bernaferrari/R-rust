g <- glm(c(1.1, 1.9, 3.2, 3.8, 5.1) ~ I(1:5))
cat(paste(round(summary.glm(g)$coefficients[, "Estimate"], 4), collapse = ","), "\n", sep = "")
