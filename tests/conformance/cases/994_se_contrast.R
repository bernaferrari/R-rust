y <- c(1.1, 1.9, 3.2, 3.8, 5.1)
g <- factor(c("a", "a", "b", "b", "b"))
fit <- aov(y ~ g)
cat(round(se.contrast(fit, list(g == "a", g == "b")), 6), "\n", sep = "")
