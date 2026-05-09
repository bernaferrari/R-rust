x <- list(a = 1)
y <- `$<-`(x, "b", 2)
print(y$a)
print(y$b)

x$b <- 3
print(x$a)
print(x$b)
