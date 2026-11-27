for (name in c('[', 'a b', 'if', '%foo%', 'a`b', 'a\nb', 'é', 'unknownDerivative')) {
  expr <- as.call(list(as.name(name), quote(x)))
  tryCatch(deriv(expr, 'x'), error = function(e) {
    dput(conditionMessage(e))
    dput(conditionCall(e))
  })
}
x <- 2
dput(eval(deriv(quote(x^2), 'x')))
