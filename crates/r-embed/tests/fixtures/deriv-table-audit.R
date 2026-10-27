functions <- c('sin','cos','tan','sinh','cosh','tanh','asin','acos','atan',
               'exp','log','sqrt','pnorm','dnorm','gamma','lgamma','digamma',
               'trigamma','psigamma','expm1','log1p','log2','log10','cospi',
               'sinpi','tanpi','factorial','lfactorial')
for (f in functions) for (argument in list(quote(x), quote(x*y), quote(y))) {
  expr <- as.call(list(as.name(f), argument))
  cat(f, ':', paste(deparse(argument), collapse=''), '\n', sep='')
  tryCatch(dput(D(expr, 'x')), error=function(e) dput(conditionMessage(e)))
}
