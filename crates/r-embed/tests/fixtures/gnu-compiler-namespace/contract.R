# Public compiler contracts checked against GNU R revision 90451.
# Run with the pinned oracle Rscript --vanilla; no port implementation is used.
user_fun <- function(x) x * 2L
f <- function(x) user_fun(x)
g <- compiler::cmpfun(f)
stopifnot(
    identical(g(3L), 6L), identical(f(3L), 6L),
    identical(formals(g), formals(f)),
    identical(environment(g), environment(f)),
    identical(body(f), quote(user_fun(x))),
    identical(compiler::cmpfun(sum), sum)
)
g <- compiler::cmpfun(function(x) x, options=list(optimize=3))
stopifnot(identical(g(7L), 7L))
for (options in list(1L, TRUE, "x", list(optimize=4), list(foo=1))) {
    g <- compiler::cmpfun(function(x) x, options=options)
    stopifnot(identical(g(7L), 7L))
}
for (value in list(1, NULL, "x", list(1), as.name("x"))) {
    error <- tryCatch(compiler::cmpfun(value), error=conditionMessage)
    stopifnot(identical(error, "cannot compile a non-function"))
}
for (call in list(
    quote(compiler::cmpfun(f=function(x) x + 1, NULL)),
    quote(compiler::cmpfun(options=NULL, function(x) x + 1)),
    quote(compiler::cmpfun(function(x) x + 1, opt=NULL))
)) {
    g <- eval(call)
    stopifnot(identical(g(2), 3))
}
duplicate <- tryCatch(
    compiler::cmpfun(function(x) x, options=NULL, opt=NULL),
    error=function(e) "duplicate rejected"
)
stopifnot(identical(duplicate, "duplicate rejected"))
cat("GNU compiler namespace contract PASS\n")
