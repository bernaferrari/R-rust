local({
  for (name in c("is.finite","is.infinite","is.nan")) {
    x<-array(c(0,Inf,NA_real_,NaN),dim=c(2L,2L),dimnames=list(c("a","b"),c("x","y")))
    attr(x,"note")<-"unchanged"
    y<-get(name)(x)
    cat(name,"shape",identical(dim(x),dim(y)),identical(dimnames(x),dimnames(y)),is.null(attr(y,"note")),"\n",sep="|")
    dimnames(y)[[1]][1]<-"changed"
    cat(name,"isolated",dimnames(x)[[1]][1],attr(x,"note"),"\n",sep="|")
    x<-asS4(c(1,Inf));y<-get(name)(x)
    cat(name,"s4-marked",isS4(x),isS4(y),paste(y,collapse=","),"\n",sep="|")
  }
  is.finite.FiniteList <- function(x) { gc(); c(dispatched=TRUE) }
  is.infinite.FiniteList <- function(x) { gc(); c(dispatched=FALSE) }
  is.nan.FiniteList <- function(x) { gc(); c(dispatched=TRUE) }
  x<-structure(list(1),class="FiniteList")
  for(name in c("is.finite","is.infinite","is.nan")) {
    y<-get(name)(x)
    cat(name,"s3",names(y),y,"\n",sep="|")
  }
  setClass("FinitePredicateBox",slots=c(x="numeric"))
  x<-new("FinitePredicateBox",x=1)
  for(name in c("is.finite","is.infinite","is.nan")) {
    message<-tryCatch({get(name)(x);"unexpected success"},error=function(e)conditionMessage(e))
    cat(name,"s4-default",message,"\n",sep="|")
  }
  setMethod("is.finite","FinitePredicateBox",function(x) {gc();c(method=TRUE)})
  setMethod("is.infinite","FinitePredicateBox",function(x) {gc();c(method=FALSE)})
  setMethod("is.nan","FinitePredicateBox",function(x) {gc();c(method=TRUE)})
  x<-new("FinitePredicateBox",x=1)
  for(name in c("is.finite","is.infinite","is.nan")) {
    y<-get(name)(x)
    cat(name,"s4-method",names(y),y,"\n",sep="|")
  }
})
