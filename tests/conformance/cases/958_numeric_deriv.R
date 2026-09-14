x <- 2
cat(as.vector(attr(numericDeriv(quote(x^2), "x"), "gradient")), "\n", sep = "")
