emit <- function(x) cat(paste(capture.output(dput(x)),collapse=" "),"\n",sep="")
for(x in list(NULL,logical(),c(FALSE,TRUE,NA),c(0L,1L,-2L,NA_integer_),c(0,-0,pi,1.23456789012345,1e-10,1e20,Inf,-Inf,NA_real_,NaN),c(0+0i,1.25-2.5i,NA_complex_),as.raw(c(0,1,255)),c("hello","é",NA_character_))) {emit(as.character(x));emit(attributes(as.character(x)))}
for(d in c(3L,7L,15L)){old<-options(digits=d);emit(as.character(c(pi,1.23456789012345,1e-10,1e20)));emit(getOption("digits"));options(old)}
x<-structure(c(1L,2L,NA_integer_),names=c("a","b","c"),note=list(1:3));y<-as.character(x);emit(y);emit(attributes(y));y[1]<-"changed";emit(x);emit(y);x[2]<-9L;emit(y);emit(x)
sx<-structure(c("a","b"),names=c("x","y"),note=list(9L));sy<-as.character(sx);emit(attributes(sy));emit(attributes(sx));sy[1]<-"new";emit(sx);emit(sy)
as.character.keep<-function(x,...)structure("method",names="kept",note=list(9L));method<-as.character(structure(0L,class="keep"));emit(method);emit(attributes(method))
for(x in list(matrix(c(0L,1L,2L,3L),2),structure(c(0L,1L),class="foo"),factor(c("a","b",NA)))){emit(as.character(x));emit(attributes(as.character(x)))}
old <- options(digits=3L)
for(s in c(-9L,0L,999L)) for(mark in c(".",",")) {options(scipen=s,OutDec=mark);emit(as.character(c(1.23456789012345,1e-8,1e20)));emit(getOption("digits"));emit(getOption("scipen"));emit(getOption("OutDec"))}
options(old)
