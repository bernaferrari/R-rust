x <- seq(0,1,length.out=30)
y <- sin(x*5)
f <- loess(y~x)
cached <- predict(f)
new <- predict(f,newdata=.2)
f$pars$span <- NaN
f$divisor <- numeric(0)
print(identical(cached,predict(f)))
print(identical(new,predict(f,newdata=.2)))
g <- loess(y~x,control=loess.control(surface='direct'))
g$pars$span <- NaN
print(tryCatch({predict(g,newdata=.2);FALSE},error=function(e)TRUE))
print(1+1)
