# Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5,
# R 4.7.0 development revision 90451; uncompressed XDR serialization v2.
# Bodies are invoked directly by the Rust GNU VM in an explicit environment.
stopifnot(getRversion() == '4.7.0')
plain <- function() {
 x<-list(a=c(1L,2L),b=99L);events<-character();rhsValue<-7L
 rhs<-function(){events<<-c(events,'rhs');x<<-list(a=c(30L,40L),b=88L);rhsValue}
 index<-function(){events<<-c(events,'index');1L}
 result<-withVisible(x$a[index()]<-rhs())
 list(result=result,x=x,events=events)
}
super <- function() {
 x<-c(1L,2L);events<-character();rhsValue<-7L
 f<-function(){x<-c(50L,60L);rhs<-function(){events<<-c(events,'rhs');rhsValue};index<-function(){events<<-c(events,'index');1L};r<-withVisible(x[index()]<<-rhs());list(result=r,local=x)}
 z<-f();list(result=z$result,local=z$local,outer=x,events=events)
}
custom <- function() {
 x<-list(a=c(1L,2L));events<-character();calls<-list();rhsValue<-7L
 `$`<-function(x,name){events<<-c(events,'get');calls[[length(calls)+1L]]<<-sys.call();.subset2(x,as.character(substitute(name)))}
 `$<-`<-function(x,name,value){events<<-c(events,'outer');calls[[length(calls)+1L]]<<-sys.call();x[[as.character(substitute(name))]]<-value;x}
 `[<-`<-function(x,i,value){events<<-c(events,'inner');calls[[length(calls)+1L]]<<-sys.call();.Primitive('[<-')(x,i,value=value)}
 rhs<-function(){events<<-c(events,'rhs');rhsValue}
 index<-function(){events<<-c(events,'index');1L}
 result<-withVisible(x$a[index()]<-rhs())
 list(result=result,x=x,events=events,calls=lapply(calls,function(call){as.list(call)}))
}
for (nm in c('plain','super','custom')) {
 f<-get(nm); cat('\n',nm,' SOURCE\n');dput(f()); cat(nm,' GNU COMPILED\n');dput(compiler::cmpfun(f)());stopifnot(identical(f(),compiler::cmpfun(f)()))
}
out <- "crates/r-embed/tests/fixtures/gnu-bytecode-nested-subassignment"
functions <- list(nested=function() x$a[index()]<-rhs(),
                  super=function() x[index()]<<-rhs())
for (name in names(functions)) {
 environment(functions[[name]]) <- baseenv()
 saveRDS(compiler::cmpfun(functions[[name]]),file.path(out,paste0(name,".rds")),version=2,compress=FALSE)
}
cat("Pinned GNU source/cmpfun nested replacement oracle passed; fixtures generated\n")
