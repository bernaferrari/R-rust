f <- function(a,b) a
tryCatch(f(a=1,b=2,c=3,d=4), error=function(e) cat("1:",conditionMessage(e),"\n"))
tryCatch(f(1,2,3,4), error=function(e) cat("2:",conditionMessage(e),"\n"))
