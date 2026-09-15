x <- 1:5
y <- c(1.1, 1.9, 3.2, 3.8, 5.1)
fit <- lm(y ~ x)
tp <- termplot(fit, plot = FALSE)
cat(paste(round(tp[[1]]$x, 4), collapse = ","), "\n", sep = "")
cat(paste(round(tp[[1]]$y, 4), collapse = ","), "\n", sep = "")
cat(round(attr(tp, "constant"), 4), "\n", sep = "")
cat(names(tp), "\n", sep = "")
cat(tryCatch(termplot(1), error = function(e) conditionMessage(e)), "\n", sep = "")
