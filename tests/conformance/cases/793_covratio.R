y <- c(1, 2, 2, 4, 5)
x <- 1:5
fit <- lm(y ~ x)
cr <- covratio(fit)
cat(paste(round(cr, 4), collapse = ","), "\n", sep = "")
