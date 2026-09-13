d <- uniroot(sin, c(3, 4))
cat(abs(d$root - pi) < 1e-4, "\n", sep = "")
cat(abs(d$f.root) < 1e-4, "\n", sep = "")
