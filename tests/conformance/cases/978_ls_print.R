fit <- lsfit(1:5, c(1.1, 1.9, 3.2, 3.8, 5.1))
out <- ls.print(fit, print.it = FALSE)
cat(paste(round(out$coef.table[[1]][, "Estimate"], 4), collapse = ","), "\n", sep = "")
