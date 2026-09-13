d <- ks.test(1:10, "punif", 0, 11)
cat(sprintf("%.8f", unname(d$statistic)), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
