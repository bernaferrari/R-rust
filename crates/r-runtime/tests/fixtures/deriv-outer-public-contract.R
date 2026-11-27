x <- 2; y <- 3; z <- 4
for (expr in list(quote((x+y)*(x+y)), quote(sin(x*y)+sin(x*y)), quote(exp(x*y)/(x+y)))) {
  ans <- deriv(expr, c('x','y','z'), tag='.tmp', hessian=TRUE)
  cat(paste(deparse(ans), collapse='\n'), '\n', sep='')
  dput(eval(ans))
}
local({
  captured <- 13
  prototype <- function(x=2,y=3) captured
  ans <- deriv3(quote(x*y+x^2), c('x','y'), prototype, tag='.d')
  cat('same environment=',identical(environment(ans),environment(prototype)),'\n',sep='')
  dput(formals(ans));dput(ans())
})
tryCatch(deriv(quote(unsupportedDerivative(x)),'x'),error=function(e)cat(conditionMessage(e),'\n',sep=''))
dput(eval(deriv(quote(x^2),'x')))
