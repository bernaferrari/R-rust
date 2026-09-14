fit <- lsfit(1:5, c(1.1, 1.9, 3.2, 3.8, 5.1))
cat(paste(round(ls.diag(fit)$hat, 4), collapse = ","), "\n", sep = "")
