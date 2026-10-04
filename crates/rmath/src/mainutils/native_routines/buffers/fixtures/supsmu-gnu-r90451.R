# GNU R devel r90451, source commit bac583951b728e97b9786804d3b4081f0fe18df5.
# Rscript --vanilla this-file.R emits the adjacent TSV exactly.
fmt <- function(x) paste(format(x,digits=17,scientific=TRUE,trim=TRUE),collapse=',')
cat('case\targument\ttype\tinput\toutput\n')
one <- function(label,x,y,w=rep(1,length(x)),iper=1L,span=0,bass=0,constant=FALSE,fixed=span>0) {
 n <- length(x)
 input <- list(as.integer(c(n,991L)),c(x,901.25,902.5),c(y,903.25,904.5),c(w,905.25,906.5),if(constant)integer() else as.integer(c(iper,992L)),if(constant)double() else c(span,993.25),if(constant || fixed)double() else c(bass,994.25),rep(3.5,n+2L),if(constant)double() else rep(7.75,if(fixed)n+2L else 7L*n+2L),if(constant || fixed)double() else c(6.25,995.25))
 out <- do.call(.Fortran,c(list(get('C_supsmu',asNamespace('stats'))),input))
 stopifnot(length(out)==10L)
 for(i in seq_along(input)){cat(label,i-1L,typeof(input[[i]]),fmt(input[[i]]),fmt(out[[i]]),sep='\t');cat('\n')}
 invisible(out)
}
x <- (0:23)/23;y <- sin(x*6)+x*.25+rep(c(.03,-.02),12)
one('cv',x,y)
one('bass',x,y,bass=8)
one('fixed',x,y,span=.3)
one('weighted',x,y,w=rep(c(0,.5,2,1),6),span=.4)
one('periodic',x,y,iper=2L,span=.3)
one('periodic-cv',x,y,iper=2L)
one('periodic-fallback',x*2,y,iper=2L,span=.3)
one('ties',rep((0:7)/7,each=3),y)
one('constant',rep(2,3),c(1,3,7),w=c(1,2,3),constant=TRUE)
one('constant-zero-weight',rep(2,1),3,w=0,constant=TRUE)
one('ignored-bass',x,y,bass=11)
