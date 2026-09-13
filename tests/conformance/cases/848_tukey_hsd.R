y <- c(1, 2, 3, 4, 5, 6)
g <- factor(c("a", "a", "b", "b", "c", "c"))
t <- TukeyHSD(aov(y ~ g))$g
cat(paste(round(as.vector(t), 4), collapse = ","), "\n", sep = "")
cat(paste(rownames(t), collapse = ","), "\n", sep = "")
