# Pinned GNU r90451, source commit bac583951b728e97b9786804d3b4081f0fe18df5.
fmt <- function(x) paste(format(x,digits=17,scientific=TRUE,trim=TRUE),collapse=',')
one <- function(label,args) {
 out <- do.call(.C,c(list(get('C_HoltWinters',asNamespace('stats'))),args))
 for(i in seq_along(args)) { cat(label,i-1L,typeof(args[[i]]),fmt(args[[i]]),fmt(out[[i]]),sep='\t'); cat('\n') }
 invisible(out)
}
a <- one('init-only',list(double(),2L,double(),double(),double(),3L,integer(),integer(),0L,0L,2,double(),double(),double(),0,double(),double()))
stopifnot(a[[15]]==2,length(a[[1]])==0L,length(a[[14]])==0L)
b <- one('init-trend-season',list(double(),2L,double(),double(),double(),3L,integer(),2L,1L,1L,2,3,c(.5,1.5),double(),0,0,c(99,99)))
stopifnot(b[[15]]==2,b[[16]]==3,identical(b[[17]],c(.5,1.5)),length(b[[14]])==0L)
c <- one('iter-disabled-components',list(c(1,2),2L,.3,double(),double(),1L,1L,0L,0L,0L,2,double(),double(),0,double(3),double(2),double()))
stopifnot(abs(c[[15]][3]-1.79)<1e-12,abs(c[[14]]-1.09)<1e-12,length(c[[4]])==0L,length(c[[5]])==0L)
