f <- function(x, y) x + y
e <- new.env()
g <- `environment<-`(f, e)

print(forceAndCall(1, sum, 1, 2))
print(declare(foo))
print(identical(environment(g), e))
print(.primTrace(sum))
print(.primUntrace(sum))
print(tryCatch(standardGeneric("foo"), error = function(e) conditionMessage(e)))
