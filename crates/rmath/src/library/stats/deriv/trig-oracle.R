stopifnot(as.character(getRversion()) == "4.7.0")
cases <- list(quote(tan(x)), quote(tan(x^2)), quote(tan(sin(x))), quote(tan(x+y)), quote(tan(y)), quote(-tan(x)), quote(tan(x)*sin(x)), quote(cos(x)), quote(sqrt(x)))
for (e in cases) {
 d <- D(e, "x")
 cat(paste(deparse(e), collapse=""), paste(deparse(d), collapse=""), typeof(d), is.null(attributes(d)), sprintf("%.17g", eval(d, list(x=.3,y=.2))), sep="\t")
 cat("\n")
}
