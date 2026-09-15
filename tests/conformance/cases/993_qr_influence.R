fit <- lm(c(1.1, 1.9, 3.2, 3.8, 5.1) ~ I(1:5))
inf <- qr.influence(qr(cbind(1, 1:5)), residuals(fit))
cat(paste(c(round(inf$hat, 4), round(as.vector(inf$sigma), 4)), collapse = ","), "\n", sep = "")
