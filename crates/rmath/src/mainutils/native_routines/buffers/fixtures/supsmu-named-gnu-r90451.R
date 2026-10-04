# Independent GNU R devel r90451 argument-matching probe.
# Source commit bac583951b728e97b9786804d3b4081f0fe18df5.
x <- (0:23)/23
y <- sin(x*6)+x*.25+rep(c(.03,-.02),12)
expected <- stats::supsmu(x,y)
stopifnot(identical(stats::supsmu(y,x=x),expected))
stopifnot(identical(stats::supsmu(sp='cv',x,y=y),expected))
cat('PASS exact before positional and partial matching\n')
