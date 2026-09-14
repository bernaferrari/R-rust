f <- factor(c("a", "b", "a", "b"))
contrasts(f) <- contr.sum(2)
cat(paste(as.vector(contrasts(f)), collapse = ","), "\n", sep = "")
