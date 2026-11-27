# oracle/r-oracle.json: GNU R bac583951b728e97b9786804d3b4081f0fe18df5
# R 4.7.0 development, revision 90451. Uncompressed XDR version 2.
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-eval-expression"
dir.create(out, recursive=TRUE, showWarnings=FALSE)
f <- function(e) eval(e)
compiled <- compiler::cmpfun(f)
saveRDS(compiled,file.path(out,"eval.rds"),version=2,compress=FALSE)
for (fun in list(f,compiled)) {
  stopifnot(identical(withVisible(fun(expression())),list(value=NULL,visible=TRUE)),
            identical(withVisible(fun(expression(1L,NULL))),list(value=NULL,visible=TRUE)),
            identical(withVisible(fun(expression(invisible(1L),NULL))),list(value=NULL,visible=TRUE)),
            identical(withVisible(fun(expression(1L,invisible(NULL)))),list(value=NULL,visible=FALSE)),
            identical(withVisible(fun(quote(return(7L)))),list(value=7L,visible=TRUE)),
            identical(withVisible(fun(quote(return(invisible(7L))))),list(value=7L,visible=FALSE)),
            identical(fun(expression(return(7L),9L)),7L))
}
stopifnot(identical((function(){eval(quote(return(7L)));9L})(),9L))
# Environment conversion and integer/logical argument semantics.
x <- 10L
stopifnot(identical(eval(quote(x),list(x=7L)),7L),
          identical(eval(quote(x),pairlist(x=8L)),8L),
          identical(eval(quote(x),list(),list2env(list(x=9L))),9L),
          identical(eval(quote(x),0L),eval(quote(x),0)))
stopifnot(inherits(try(eval(1L,TRUE),silent=TRUE),"try-error"))
