stopifnot(as.character(getRversion()) == "4.7.0")
cases <- list(quote(psigamma(x)),quote(psigamma(x^2)),quote(psigamma(x,0)),quote(psigamma(x,1)),quote(psigamma(x,2)),quote(psigamma(x,y)))
for (e in cases) {
 d <- D(e,"x")
 cat(paste(deparse(e),collapse=""),paste(deparse(d),collapse=""),typeof(d),is.null(attributes(d)),sprintf("%.17g",eval(d,list(x=1.3,y=2))),sep="\t")
 cat("\n")
}
