make <- function(x) function(y) x + y
add2 <- make(2)
add2(40)
