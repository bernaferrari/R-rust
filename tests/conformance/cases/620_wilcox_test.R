d <- wilcox.test(1:5, 3:7)
cat(sprintf("%.8f", unname(d$statistic)), "\n", sep = "")
cat(is.numeric(d$p.value) && d$p.value > 0 && d$p.value < 1, "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
