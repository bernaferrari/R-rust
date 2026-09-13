y <- c(1, 2, 2, 4, 5)
x <- 1:5
inf <- lm.influence(lm(y ~ x), do.coef = FALSE)
cat(paste(round(inf$hat, 4), collapse = ","), "\n", sep = "")
cat(paste(round(inf$sigma, 4), collapse = ","), "\n", sep = "")
cat(paste(round(inf$wt.res, 4), collapse = ","), "\n", sep = "")
