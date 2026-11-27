emit <- function(x) cat(paste(capture.output(dput(x)), collapse=" "), "\n", sep="")
observe <- function(x) { events <- character(); value <- withCallingHandlers(as.double(x), warning=function(w) {events <<- c(events, conditionMessage(w)); gc(); stopifnot(identical(as.double(2L),2)); invokeRestart("muffleWarning")}); emit(value); emit(events) }
for (x in list(logical(), c(FALSE,TRUE,NA), c(0L,1L,-2L,NA_integer_), c(0,-0,1.9,-1.9,Inf,-Inf,NA_real_,NaN), c(0+0i,1.9+0i,-1.9+2i,NA_complex_), complex(real=c(NaN,Inf,2),imaginary=c(0,0,NaN)), c("0","-1.9","0x10","1e2","NaN","Inf","-Inf","1e309","", "bad",NA_character_), as.raw(c(0,1,255)))) observe(x)
x <- structure(c(0L,2L,NA_integer_), names=c("a","b","c"), note=list(1:3)); y <- as.double(x); emit(y); emit(attributes(y)); y[1] <- 7; emit(x); emit(y); x[2] <- 0L; emit(y); emit(x)
for (x in list(matrix(c(0L,1L,NA_integer_,2L),2), structure(c(0L,1L), class="foo"), list(0L,1L,NA_integer_), pairlist(0L,1L,NA_integer_))) observe(x)
old <- options(warn=2); message <- tryCatch(as.double("bad"), error=conditionMessage); options(old); emit(message); emit(as.double(2L))
