x <- c(1, 2, 3, 2, 1, 2, 3, 2, 1)
a <- arima0(x, order = c(1, 0, 0), method = "CSS")
cat(paste(round(as.numeric(a$coef), 4), collapse = ","), "\n", sep = "")
cat(class(a), "\n", sep = "")
