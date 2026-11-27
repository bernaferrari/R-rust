local({
  inputs<-list(named=c(a=4,b=1,c=1),all_na=c(a=NA_real_,b=NaN),inf=c(a=Inf,b=-Inf,c=-Inf),logical=c(a=NA,b=TRUE,c=FALSE),character=c(a="2",b="1"),bad_character=c(a="bad",b="3"),complex=c(a=2+4i,b=1+5i),list=list(a=2,b=1),bad_list=list(a=1:2),null=NULL,empty=integer(0),raw=as.raw(c(9,1)),one_row_frame=data.frame(a=3,b=1),two_row_frame=data.frame(a=c(3,1)),matrix=matrix(c(3,1,2,1),2),factor=structure(factor(c("c","a","b")),names=c("a","b","c")),ordered=structure(ordered(c("c","a","b")),names=c("a","b","c")))
  for(name in c("which.min","which.max")) {
    for(label in names(inputs)) {
      warnings<-character()
      result<-tryCatch(withCallingHandlers({y<-get(name)(inputs[[label]]);paste(typeof(y),length(y),paste(unname(y),collapse=","),paste(names(y),collapse=","),sep="|")},warning=function(w){gc();warnings<<-c(warnings,conditionMessage(w));invokeRestart("muffleWarning")}),error=function(e)paste("ERROR",conditionMessage(e),sep="|"))
      cat(name,label,result,paste(warnings,collapse=";"),"\n",sep="|")
    }
  }
  assign("xtfrm.ExtremaOrder", function(x){gc();structure(c(30,10,20),names=c("transformed-a","transformed-b","transformed-c"))}, envir=.GlobalEnv)
  x<-structure(c(a=1,b=2,c=3),class="ExtremaOrder")
  for(name in c("which.min","which.max")){y<-get(name)(x);cat(name,"xtfrm",y,names(y),paste(unclass(x),collapse=","),"\n",sep="|")}
  x<-structure(c(a=3,b=1,c=2),class="Date")
  for(name in c("which.min","which.max")){y<-get(name)(x);cat(name,"date",y,paste(names(y),collapse=","),"\n",sep="|")}
  for(kind in c("list","expression")) for(mode in c("logical","integer","double","complex","raw")) {
    x<-if(kind=="list")list(1:2) else expression(1:2)
    message<-tryCatch({as.vector(x,mode);"unexpected success"},error=function(e)conditionMessage(e))
    cat("coerce",kind,mode,message,"\n",sep="|")
  }
})
