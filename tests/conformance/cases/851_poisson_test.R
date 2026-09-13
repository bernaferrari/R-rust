p <- poisson.test(10, alternative = "greater")
cat(round(p$p.value * 1e7, 4), "\n", sep = "")
cat(unname(p$estimate), "\n", sep = "")
