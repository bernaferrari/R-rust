local({
  cat("wrapper",is.primitive(tabulate),identical(names(formals(tabulate)),c("bin","nbins")),"\n",sep="|")
  cases<-list(valid=function()tabulate(c(1L,2L,2L,0L,-1L,NA_integer_),3L),empty=function()tabulate(integer(0),3L),default_empty=function()tabulate(integer(0)),default_factor=function()tabulate(factor(c("a","b","b"))),default_real=function()tabulate(c(1.1,2.9)),default_missing=function()tabulate(c(NA_real_,Inf)),null=function()tabulate(NULL,3L),logical=function()tabulate(TRUE,3L),character=function()tabulate("1",3L),factor=function()tabulate(factor(c("b","a","b")),3L),real=function()tabulate(c(1.9,2.1,NA_real_,NaN,Inf,-1),3L),negative=function()tabulate(1L,-1L),zero=function()tabulate(1L,0L),fraction=function()tabulate(c(1L,2L),2.8),na_nbins=function()tabulate(1L,NA_integer_),null_nbins=function()tabulate(1L,NULL),vector_nbins=function()tabulate(1L,c(2L,3L)),logical_nbins=function()tabulate(c(1L,2L),TRUE),character_nbins=function()tabulate(1L,"2"),large_nbins=function()tabulate(1L,2^31))
  for(label in names(cases)) {
    warnings<-character()
    result<-tryCatch(withCallingHandlers({x<-cases[[label]]();paste(typeof(x),length(x),paste(x,collapse=","),sep="|")},warning=function(w){gc();warnings<<-c(warnings,conditionMessage(w));invokeRestart("muffleWarning")}),error=function(e)paste("ERROR",conditionMessage(e),sep="|"))
    cat(label,result,paste(warnings,collapse=";"),"\n",sep="|")
  }
  internal<-list(real=function().Internal(tabulate(c(1,2),3L)),logical=function().Internal(tabulate(TRUE,3L)),null=function().Internal(tabulate(NULL,3L)),negative=function().Internal(tabulate(1:3,-1L)),missing_size=function().Internal(tabulate(1:3,NULL)),na_size=function().Internal(tabulate(1:3,NA_integer_)),vector_size=function().Internal(tabulate(1:3,c(2L,3L))),character_size=function().Internal(tabulate(1:3,"2")),missing_argument=function().Internal(tabulate(1:3)),extra_argument=function().Internal(tabulate(1:3,2L,3L)))
  for(label in names(internal)) {
    result<-tryCatch({x<-internal[[label]]();paste(typeof(x),length(x),paste(x,collapse=","),sep="|")},error=function(e)paste("ERROR",conditionMessage(e),sep="|"))
    cat("internal",label,result,"\n",sep="|")
  }
  x<-structure(c(1L,2L,2L),names=c("a","b","c"),note="kept")
  y<-tabulate(x,3L);y[1]<-9L
  cat("isolation",is.null(attributes(y)),paste(x,collapse=","),paste(names(x),collapse=","),attr(x,"note"),"\n",sep="|")
})
