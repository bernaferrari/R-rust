f <- function(x) x
class(f) <- "stepfun"
cat(is.stepfun(f), "\n", sep = "")
cat(is.stepfun(1:3), "\n", sep = "")
