ss <- selfStart(function(x, A) A * x, function(mCall, data, LHS, ...) c(A = 2), "A")
cat(as.vector(getInitial(ss, list())), "\n", sep = "")
