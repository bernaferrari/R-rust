d <- Box.test(1:10)
cat(sprintf("%.8f", unname(d$statistic)), "\n", sep = "")
cat(sprintf("%.8f", d$p.value), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
