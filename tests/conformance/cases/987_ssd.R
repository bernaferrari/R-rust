fit <- lm(c(1.1, 1.9, 3.2, 3.8, 5.1) ~ I(1:5))
class(fit) <- c("mlm", class(fit))
cat(round(as.vector(SSD(fit)$SSD), 4), "\n", sep = "")
