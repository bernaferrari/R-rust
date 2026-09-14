cat("[", naprint(1:3), "]\n", sep = "")
x <- 1:3
class(x) <- "omit"
cat(naprint(x), "\n", sep = "")
y <- 1
class(y) <- "omit"
cat(naprint(y), "\n", sep = "")
