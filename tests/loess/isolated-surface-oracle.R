# Run with pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5.
# No port output is used to create these pruned native interpolation surfaces.
args <- commandArgs(TRUE)
stopifnot(length(args) %in% 1:2)
number <- function(x) if (is.integer(x)) paste(x,collapse=",") else
    paste(format(x, digits=17, scientific=TRUE, trim=TRUE), collapse=",")
array <- function(x) paste0("[", number(x), "]")
cases <- vapply(1:4, function(d) {
    n <- 40L
    x <- outer(seq_len(n), c(7,13,19,29)[seq_len(d)], function(i,p) ((i*p) %% 97)/97)
    colnames(x) <- paste0("x", seq_len(d))
    y <- sin(5*x[,1]) + rowSums(x*x) + .03*cos(seq_len(n))
    f <- loess(y~., data=data.frame(y,x), degree=1L, span=.85, normalize=FALSE,
               control=loess.control(statistics="none",cell=.2))
    q <- x[c(3,17,29),,drop=FALSE]
    k <- f$kd
    paste0('{"d":',d,',"n":',n,',"x":',array(x),',"y":',array(y),
           ',"queries":',array(q),',"fit":',array(predict(f,newdata=as.data.frame(q))),
           ',"parameter":',array(k$parameter),',"a":',array(k$a),
           ',"xi":',array(k$xi),',"vert":',array(k$vert),',"vval":',array(k$vval),'}')
}, "")
cat('{"oracle_source_commit":"bac583951b728e97b9786804d3b4081f0fe18df5",',
    '"cases":[',paste(cases,collapse=","),']}\n',file=args[[1]],sep="")
if (length(args) == 2L) {
    x <- seq(0,1,length.out=30)
    y <- sin(5*x)
    f <- loess(y~x)
    q <- c(.15,.45,.75)
    saveRDS(list(model=f,q=q,expected=predict(f,q)),args[[2]],version=3,compress=FALSE)
}
