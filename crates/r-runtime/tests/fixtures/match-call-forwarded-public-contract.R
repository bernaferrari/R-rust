capture<-function(...)list(match.call(),match.call(expand.dots=FALSE))
forward<-function(...)capture(...)
mc<-forward(a=)
stopifnot(identical(mc[[1]],quote(capture(a=..1))),identical(mc[[2]]$...,pairlist(a=as.name("..1"))))
mc<-capture(a=)
stopifnot(identical(mc[[1]],quote(capture(a=))),identical(mc[[2]]$...,as.pairlist(alist(a=))))
count<-0L
mc<-forward(a=,b={count<<-count+1L;stop("must not force")},c=7)
stopifnot(count==0L,identical(mc[[1]],quote(capture(a=..1,b=..2,c=7))),identical(mc[[2]]$...,pairlist(a=as.name("..1"),b=as.name("..2"),c=7)))
mc<-forward(a=NULL,b=TRUE,c="str")
stopifnot(identical(mc[[1]],quote(capture(a=..1,b=TRUE,c="str"))))
marker<-42L;mc<-forward(a=marker)
stopifnot(identical(mc[[1]],quote(capture(a=..1))))
f<-function(...){val<-match.call(expand.dots=FALSE)$...;x<-val[[1]];eval.parent(substitute(missing(x)))}
g<-function(...)h(f(...));h<-function(...)list(...);k<-function(...)g(...)
stopifnot(identical(k(a=),list(TRUE)))
TRUE
