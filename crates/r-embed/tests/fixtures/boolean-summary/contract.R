cases <- list(
 empty=logical(), null=NULL, logical=c(FALSE,NA,TRUE), missing=c(NA,NA),
 integer=c(0L,NA_integer_,2L), real=c(0,NA_real_,Inf),
 complex=c(0+1i,NA_complex_), complex_missing=c(0+0i,NA_complex_),
 raw=as.raw(c(0,255)), text=c("False","True"), text_invalid=c("tRue"," TRUE"),
 scalar_list=list(TRUE,FALSE), empty_scalar=list(integer()), null_scalar=list(NULL),
 nested_scalar=list(list(TRUE)), long_scalar=list(1:2),
 expression=expression(TRUE), pairlist=pairlist(TRUE,FALSE),
 symbol=quote(x), language=quote(f(1)), closure=function()1,
 empty_environment=new.env(parent=emptyenv())
)
emit <- function(label, code) {
 warnings <- character()
 answer <- tryCatch(withCallingHandlers(eval(code),warning=function(w) { gc(); warnings <<- c(warnings,conditionMessage(w)); invokeRestart("muffleWarning") }),error=function(e) paste0("ERROR:",conditionMessage(e)))
 cat(paste(label, paste(answer,collapse=","),paste(warnings,collapse="/"),sep="|"),"\n",sep="")
}
for(n in names(cases)) for(op in c("any","all")) for(flag in c(FALSE,TRUE)) {
 emit(paste(n,op,flag,sep=":"),substitute(FUN(cases[[NAME]],na.rm=FLAG),list(FUN=as.name(op),NAME=n,FLAG=flag)))
}
flags <- list(NA,NULL,logical(),2L,-2L,"fAlSe",c(TRUE,FALSE))
for(i in seq_along(flags)) for(op in c("any","all")) {
 emit(paste("flag",i,op,sep=":"),substitute(FUN(NA,na.rm=FLAG),list(FUN=as.name(op),FLAG=flags[[i]])))
}
emit("noargs:any",quote(any()))
emit("noargs:all",quote(all()))
emit("short:any",quote(any(TRUE,"bad",list(NULL))))
emit("short:all",quote(all(FALSE,"bad",list(NULL))))
emit("order:any",quote(any(NA,FALSE,TRUE)))
emit("order:all",quote(all(NA,TRUE,FALSE)))
emit("duplicate:any",quote(any(NA,na.rm=FALSE,na.rm=TRUE)))
emit("duplicate:all",quote(all(NA,na.rm=FALSE,na.rm=TRUE)))
Summary.summary_probe <- function(...,na.rm=FALSE) { gc(); if(.Generic=="any") TRUE else FALSE }
emit("method:any",quote(any(structure(0,class="summary_probe"),na.rm=TRUE)))
emit("method:all",quote(all(structure(1,class="summary_probe"),na.rm=TRUE)))
rm(Summary.summary_probe)
