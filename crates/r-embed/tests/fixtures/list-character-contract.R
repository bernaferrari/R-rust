x <- list(a=.25, b=1:2, c=NA_real_, d=NULL, e=TRUE, f='abc', g=quote(foo+1))
stopifnot(identical(x == as.character(x), setNames(rep(TRUE, 7), names(x))))
stopifnot(identical('0.25' == x, setNames(c(TRUE, rep(FALSE, 6)), names(x))))
stopifnot(identical(list(NA_character_) == NA_character_, NA))
stopifnot(identical(list() == character(), logical()))
stopifnot(identical(expression(1, x+1) == c('1', 'x + 1'), c(TRUE, TRUE)))
a <- array(list(1, 2, 3, 4), c(2L, 2L), dimnames=list(c('r1','r2'),c('c1','c2')))
expected <- matrix(TRUE, 2, 2, dimnames=dimnames(a))
stopifnot(identical(a == c('1','2','3','4'), expected))
stopifnot(identical(list('é') == 'é', TRUE))
stopifnot(identical(tryCatch(pairlist(a=1) == "1", error=function(e) conditionMessage(e)),
                    "comparison (==) is possible only for atomic and list types"))
nonconform <- array(list(1,2),c(2L,1L))
stopifnot(identical(tryCatch(nonconform == array(rep("1",4),c(2L,2L)),
                             error=function(e) conditionMessage(e)), "non-conformable arrays"))
stopifnot(identical(tryCatch(nonconform == rep("1",4), error=function(e) conditionMessage(e)),
                    "dims [product 2] do not match the length of object [4]"))
stopifnot(identical(tryCatch(rep("1",4) == nonconform, error=function(e) conditionMessage(e)),
                    "dims [product 2] do not match the length of object [4]"))
warns <- character()
recycled <- withCallingHandlers(x[1:3] == c("0.25", "1:2"), warning=function(w) {
  warns <<- c(warns, conditionMessage(w)); invokeRestart("muffleWarning")
})
stopifnot(identical(recycled, c(a=TRUE,b=TRUE,c=FALSE)),
          identical(warns, "longer object length is not a multiple of shorter object length"))
y <- c(1,4,2,5,4,6,5,8)
out <- supsmu(1:8, y, span=list(0.25))
stopifnot(identical(round(out$y, 6), c(1.8,2.5,3.2,4.2,4.4,5.6,6.3,7)))
trace <- character()
Ops.spanprobe <- function(e1,e2) { trace <<- c(trace,.Generic); do.call(.Generic,list(unclass(e1),e2)) }
as.double.spanprobe <- function(x,...) { trace <<- c(trace,'double'); .75 }
out <- supsmu(1:8, y, span=structure(.25,class='spanprobe'))
stopifnot(identical(trace, c('==','<','>','double')))
stopifnot(identical(round(out$y, 6), c(1.928571,2.571429,3.214286,3.857143,
                                          4.857143,5.535714,6.214286,6.892857)))
cat('list comparison and supsmu: PASS\n')
