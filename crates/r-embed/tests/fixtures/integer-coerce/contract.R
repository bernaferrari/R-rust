emit <- function(x) cat(paste(capture.output(dput(x)), collapse=" "), "\n", sep="")
observe <- function(x) { events <- character(); value <- withCallingHandlers(as.integer(x), warning=function(w) {events <<- c(events, conditionMessage(w)); gc(); stopifnot(identical(as.integer(2.9),2L)); invokeRestart("muffleWarning")}); emit(value); emit(events) }
for (x in list(logical(), c(FALSE,TRUE,NA), c(0,-0,1.9,-1.9,Inf,-Inf,NA_real_,NaN,2147483647,2147483647.9,2147483648,-2147483648), c(0+0i,1.9+0i,-1.9+2i,NA_complex_), c("0","-1.9","0x10","1e2","", "bad",NA_character_), as.raw(c(0,1,255)))) observe(x)
observe(complex(real=c(2147483647,2147483647.9,2147483648,2147483648.5,-2147483647,-2147483648),imaginary=0))
x <- structure(c(0,2,NA_real_,NaN), names=c("a","b","c","d"), note=list(1:3)); y <- as.integer(x); emit(y); emit(attributes(y)); y[1] <- 7L; emit(x); emit(y); x[2] <- 0; emit(y); emit(x)
for (x in list(matrix(c(0,1,NA,2),2), structure(c(0,1), class="foo"), list(0,1,NA), pairlist(0,1,NA))) observe(x)
old <- options(warn=2); message <- tryCatch(as.integer(c(0,Inf)), error=conditionMessage); options(old); emit(message); emit(as.integer(2.9))
