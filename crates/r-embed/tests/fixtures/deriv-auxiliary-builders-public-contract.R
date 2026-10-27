x <- 2; y <- 3
for (h in c(FALSE, TRUE)) {
  ans <- deriv(quote(x*y+x^2), c('x','y'), hessian=h)
  cat('hessian=', h, '\n', sep='')
  cat(paste(deparse(ans), collapse='\n'), '\n', sep='')
  value <- eval(ans)
  dput(value)
}
f <- deriv3(quote(x*y+x^2), c('x','y'), function(x=2,y=3) NULL)
cat('closure=', typeof(f), '; environment=', environmentName(environment(f)), '\n', sep='')
dput(formals(f)); dput(f())
g <- deriv3(quote(x*y+x^2), c('x','y'), function.arg=TRUE)
cat('generated closure=', typeof(g), '; environment=', environmentName(environment(g)), '\n', sep='')
dput(formals(g)); dput(g(2,3))
