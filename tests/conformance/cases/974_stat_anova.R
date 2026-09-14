tab <- cbind(Df = c(NA, 1), Deviance = c(NA, 10))
cat(paste(round(stat.anova(as.data.frame(tab), test = "Chisq", scale = 1, df.scale = 3, n = 5)[, "Pr(>Chi)"], 6), collapse = ","), "\n", sep = "")
