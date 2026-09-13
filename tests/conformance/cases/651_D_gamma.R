cat(deparse(D(expression(gamma(x)), "x")), "\n", sep = "")
cat(deparse(D(expression(lgamma(x)), "x")), "\n", sep = "")
cat(deparse(D(expression(log1p(x)), "x")), "\n", sep = "")
cat(tryCatch(D(expression(atanh(x)), "x"), error = function(e) conditionMessage(e)), "\n", sep = "")
