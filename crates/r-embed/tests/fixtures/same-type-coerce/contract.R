emit <- function(x) {if(is.complex(x)) x<-list(real=unname(Re(x)),imaginary=unname(Im(x)),attributes=attributes(x));cat(paste(capture.output(dput(x)),collapse=" "),"\n",sep="")}
convert <- function(x,kind,api) if(api=="atomic")switch(kind,logical=as.logical(x),integer=as.integer(x),double=as.double(x),complex=as.complex(x),raw=as.raw(x),character=as.character(x))else as.vector(x,kind)
values <- list(logical=c(FALSE,TRUE,NA),integer=c(1L,NA_integer_,3L),double=c(1.25,NA_real_,Inf),complex=c(1+2i,NA_complex_,3-4i),raw=as.raw(c(0,127,255)),character=c("a",NA_character_,"é"))
for(kind in names(values)) for(api in c("atomic","vector")) {
 x<-structure(values[[kind]],names=c("one","two","three"),class="unknown",note=list(1:3))
 y<-convert(x,kind,api)
 emit(y);emit(attributes(y));emit(attributes(x));y[1]<-y[2];emit(x);x[3]<-x[1];emit(y)
}
for(kind in names(values))for(api in c("atomic","vector")){x<-values[[kind]];y<-convert(x,kind,api);emit(y);emit(attributes(y));y[1]<-y[2];emit(x);x[3]<-x[1];emit(y)}
