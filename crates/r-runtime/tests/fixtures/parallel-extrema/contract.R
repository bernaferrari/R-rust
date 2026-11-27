local({
 cases<-list(single=list(c(a=1,b=NA_real_)),logical=list(c(TRUE,NA),c(FALSE,TRUE)),integer=list(c(3L,NA_integer_,1L),c(2L,4L,1L)),real=list(c(3,NA_real_,NaN,Inf),c(2,4,1,-Inf)),nan_first=list(NaN,NA_real_),na_first=list(NA_real_,NaN),all_nan=list(NaN,NaN),last_value=list(NA_real_,NaN,1),zero_left=list(numeric(0),1:3),zero_right=list(1:3,numeric(0)),null=list(NULL,1:3),zero_order=list(integer(0),1L,2.5),all_zero=list(integer(0),numeric(0)),recycle=list(1:3,c(3L,1L)),names=list(c(a=3,b=1),c(2,4)),matrix=list(matrix(1:4,2),3),complex=list(1+2i,2),raw=list(as.raw(2),1),no_args=list())
 render<-function(x)paste(typeof(x),length(x),paste(ifelse(is.na(x),ifelse(is.nan(x),"NaN","NA"),as.character(x)),collapse=","),paste(names(x),collapse=","),paste(dim(x),collapse=","),sep="|")
 for(f in c("pmin","pmax")) for(rm in c(FALSE,TRUE)) for(label in names(cases)) {
  warnings<-character()
  value<-tryCatch(withCallingHandlers(render(do.call(f,c(cases[[label]],list(na.rm=rm)))),warning=function(w){gc();warnings<<-c(warnings,conditionMessage(w));invokeRestart("muffleWarning")}),error=function(e)paste("ERROR",conditionMessage(e),sep="|"))
  cat(f,rm,label,value,paste(warnings,collapse=";"),"\n",sep="|")
 }
 for(f in c("pmin","pmax")) {
  value<-tryCatch(do.call(f,list(1,2,na.rm=NA)),error=function(e)conditionMessage(e))
  cat(f,"invalid_na_rm",value,"\n",sep="|")
  value<-do.call(paste0(f,".int"),list(c(a=3,b=1),c(2,4)))
  cat(f,"int_attrs",render(value),"\n",sep="|")
 }
})
