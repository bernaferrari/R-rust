y <- c(1, 2, 2, 4, 5)
x <- 1:5
g <- getCall(lm(y ~ x))
cat(as.character(g[[1]]), "\n", sep = "")
cat(deparse(g[[2]]), "\n", sep = "")
cat(is.null(model.offset(lm(y ~ x))), "\n", sep = "")
