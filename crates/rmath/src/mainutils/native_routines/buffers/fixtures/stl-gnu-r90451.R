# GNU R devel r90451, source commit bac583951b728e97b9786804d3b4081f0fe18df5.
# Rscript --vanilla this-file.R emits the adjacent TSV exactly.
fmt <- function(x) paste(format(x,digits=17,scientific=TRUE,trim=TRUE),collapse=',')
cat('case\targument\ttype\tinput\toutput\n')
one <- function(label,y,p=4L,spans=c(7L,9L,7L),degrees=c(1L,1L,1L),jumps=c(1L,1L,1L),inner=2L,outer=0L,n=length(y),unread=FALSE) {
 data <- if(unread) double() else c(y,901.25,902.5)
 input <- c(list(data,as.integer(c(n,991)),as.integer(c(p,992))),lapply(spans,function(x)as.integer(c(x,993))),lapply(degrees,function(x)as.integer(c(x,994))),lapply(jumps,function(x)as.integer(c(x,995))),list(as.integer(c(inner,996)),as.integer(c(outer,997)),rep(2.25,n+2L),if(unread)double() else rep(3.5,n+2L),rep(7.75,n+2L)))
 out <- do.call(.Fortran,c(list(get('C_stl',asNamespace('stats'))),input))
 stopifnot(length(out)==17L)
 for(i in seq_along(input)){cat(label,i-1L,typeof(input[[i]]),fmt(input[[i]]),fmt(out[[i]]),sep='\t');cat('\n')}
 invisible(out)
}
y <- 10+(0:23)*.25+rep(c(1,-1,.5,-.5),6)
one('additive',y)
one('robust',replace(y,11,y[11]+15),outer=2L)
one('jumps',y,degrees=c(0L,1L,1L),jumps=c(2L,3L,2L))
one('normalized',y[1:12],p=1L,spans=c(2L,-1L,0L),degrees=c(-1L,0L,2L),jumps=c(3L,1L,2L))
one('no-inner',double(),inner=0L,n=4L,unread=TRUE)
one('negative-iterations',double(),inner=-1L,outer=-2L,n=4L,unread=TRUE,jumps=c(0L,-1L,0L))
one('no-inner-robust',y[1:8],inner=0L,outer=1L)
one('zero-size',double(),inner=0L,n=0L,unread=TRUE)
one('minimal-cycle',c(1,2),p=2L,inner=1L)
