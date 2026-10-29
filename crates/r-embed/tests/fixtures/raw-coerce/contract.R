emit <- function(x) cat(paste(capture.output(dput(x)), collapse=" "), "\n", sep="")
observe <- function(x) { events <- character(); value <- withCallingHandlers(as.raw(x), warning=function(w) {events <<- c(events, conditionMessage(w));gc();stopifnot(identical(as.integer(as.raw(2L)),2L));invokeRestart("muffleWarning")});emit(typeof(value));emit(as.integer(value));emit(attributes(value));emit(events) }
for (x in list(NULL,logical(),c(FALSE,TRUE,NA),c(-1L,0L,1L,255L,256L,NA_integer_),c(-1.9,-.9,0,255.9,256,Inf,-Inf,NA_real_,NaN,2147483648),c(0+0i,3+2i,256+0i,NA_complex_),c("0","255","256","0xff","-1.9","bad","",NA_character_),as.raw(c(0,1,255)))) observe(x)
x<-structure(c(0L,2L,3L),names=c("a","b","c"),note=list(1:3));y<-as.raw(x);emit(as.integer(y));emit(attributes(y));y[1]<-as.raw(7L);emit(x);emit(as.integer(y));x[2]<-0L;emit(as.integer(y));emit(x)
for(x in list(matrix(c(0L,1L,2L,3L),2),structure(c(0L,1L),class="foo"),list(0L,1L,2L),pairlist(0L,1L,2L)))observe(x)
rx<-structure(as.raw(c(1L,2L,255L)),names=c("x","y","z"),note=list(9L));ry<-as.raw(rx);emit(attributes(ry));emit(attributes(rx));ry[1]<-as.raw(7L);emit(as.integer(rx));emit(as.integer(ry));rx[2]<-as.raw(9L);emit(as.integer(ry))
as.raw.keep<-function(x,...)structure(as.raw(3L),names="method",note=list(9L));method<-as.raw(structure(0L,class="keep"));emit(attributes(method));emit(as.integer(method))
old<-options(warn=2);message<-tryCatch(as.raw(256L),error=conditionMessage);options(old);emit(message);observe(2L)
