d <- deriv3(~x^2, "x")
x <- 3
v <- eval(d)
cat(sprintf("%.8f", as.vector(v)), "\n", sep = "")
cat(sprintf("%.8f", as.vector(attr(v, "gradient"))), "\n", sep = "")
cat(sprintf("%.8f", as.vector(attr(v, "hessian"))), "\n", sep = "")
