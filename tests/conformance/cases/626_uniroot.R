d <- uniroot(function(x) x - 1, c(0, 2))
cat(sprintf("%.10f", d$root), "\n", sep = "")
cat(sprintf("%.10f", d$f.root), "\n", sep = "")
