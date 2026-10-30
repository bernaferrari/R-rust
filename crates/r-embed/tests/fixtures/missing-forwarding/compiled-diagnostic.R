probe <- function(x) missing(x)
forward <- function(x) c(missing(x), probe(x))
defaulted <- function(x=7) c(missing(x), probe(x))
dots <- function(...) c(missing(..1), probe(..1))
nested <- function(x) forward(x)
for (compiled in c(FALSE, TRUE)) {
  f <- if (compiled) compiler::cmpfun(forward) else forward
  d <- if (compiled) compiler::cmpfun(defaulted) else defaulted
  v <- if (compiled) compiler::cmpfun(dots) else dots
  n <- if (compiled) compiler::cmpfun(nested) else nested
  for (call in list(quote(f()), quote(f(9)), quote(d()), quote(d(9)), quote(v()), quote(v(9)), quote(n()), quote(n(9)))) {
    result <- tryCatch(paste(as.character(eval(call)), collapse=','), error=function(e) paste0('ERROR:', conditionMessage(e)))
    cat(compiled, '|', deparse(call), '|', result, '\n', sep='')
  }
}
