for (e in list(quote(x^-2), quote(x^y), quote(0^x), quote(1^x),
               quote((x+1)^(x*2)), quote(x^2), quote(y^x))) {
  dput(e)
  dput(D(e, 'x'))
  dput(D(e, 'y'))
}
x <- 2; y <- 3
dput(eval(deriv(quote(x^y), c('x', 'y'))))
for (exponent in list(numeric(), NA_real_, TRUE, c(2,3))) {
  e <- as.call(list(as.name('^'), as.name('x'), exponent))
  dput(D(e, 'x'))
}
