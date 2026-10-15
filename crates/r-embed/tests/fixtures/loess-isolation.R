x <- seq(0,1,length.out=30)
y <- sin(5*x)
q <- c(.15,.45,.75)
for (family in c("gaussian","symmetric")) {
    f <- loess(y~x,family=family)
    before <- predict(f,newdata=q)
    w <- cos(3*x)
    z <- 10+w
    g <- loess(z~x+w,degree=1,span=1)
    invisible(gc())
    stopifnot(identical(before,predict(f,newdata=q)))
    saved <- unserialize(serialize(f,NULL))
    rm(f,g)
    invisible(gc())
    stopifnot(identical(before,predict(saved,newdata=q)))
}
TRUE
