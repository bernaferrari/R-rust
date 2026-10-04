# GNU R devel r90451, source commit bac583951b728e97b9786804d3b4081f0fe18df5.
# Rscript --vanilla this-file.R emits the adjacent TSV exactly.
fmt <- function(x) paste(format(x,digits=17,scientific=TRUE,trim=TRUE),collapse=',')
cat('case\tx\ty\tweights\tspan\tperiodic\tbass\toutput_x\toutput_y\n')
one <- function(label,x,y,w=rep(1,length(x)),span=0,periodic=FALSE,bass=0){
 out <- suppressWarnings(stats::supsmu(x,y,wt=w,span=span,periodic=periodic,bass=bass))
 cat(label,fmt(x),fmt(y),fmt(w),fmt(span),as.integer(periodic),fmt(bass),fmt(out$x),fmt(out$y),sep='\t');cat('\n')
}
x <- (0:23)/23;y <- sin(x*6)+x*.25+rep(c(.03,-.02),12)
one('reverse',rev(x),rev(y))
one('ties',rep((0:7)/7,each=3),y,w=rep(c(1,.5,2),8),span=.3)
y[3] <- NaN;w <- rep(1,24);w[7] <- Inf
one('finite-filter',rev(x),rev(y),rev(w))
