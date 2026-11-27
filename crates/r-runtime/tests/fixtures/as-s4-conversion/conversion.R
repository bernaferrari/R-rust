local({
  emit <- function(...) cat(paste(..., collapse="|"), "\n", sep="")
  values <- list(logical=c(TRUE,NA), integer=c(1L,2L), real=c(1,NA_real_), complex=c(1+2i,NA_complex_), character=c("a",NA_character_), raw=as.raw(c(0,255)), list=list(a=1L), expression=expression(x+1), pairlist=pairlist(a=1L), language=quote(f(x)), closure=function(x)x, matrix=matrix(1:4,2), tagged=structure(c(1L,2L),class="legacy",note="kept"))
  for (name in names(values)) {
    for (complete in c(0L,1L,2L,-1L)) {
      x <- asS4(values[[name]])
      y <- asS4(x,FALSE,complete)
      emit(name,complete,isS4(x),isS4(y),typeof(y),paste(class(y),collapse=","),is.object(y),identical(attr(x,"note"),attr(y,"note")))
    }
  }
  x<-asS4(structure(c(1L,2L),note=list(a=7L))); alias<-x
  y<-asS4(x,FALSE); y[1]<-9L; attr(y,"note")$a<-8L
  emit("isolation",isS4(x),isS4(alias),x[1]==1L,attr(x,"note")$a==7L,y[1]==9L)
  setClass("ConversionNumbers",contains="numeric")
  x<-new("ConversionNumbers",c(1,2)); y<-asS4(x,FALSE)
  emit("numeric-class",isS4(x),isS4(y),identical(y,c(1,2)),is.null(attributes(y)))
  setOldClass(c("ConversionOld","list"))
  setClass("ConversionFromOld",contains="ConversionOld")
  x<-new("ConversionFromOld",list(a=1L)); y<-asS4(x,FALSE)
  emit("old-class",isS4(x),isS4(y),paste(class(y),collapse=","),is.null(attr(y,".S3Class")),y$a)
  setClass("ConversionSlots",slots=c(n="integer"))
  x<-new("ConversionSlots",n=4L)
  for (complete in c(0L,1L,2L,-1L)) {
    value<-tryCatch({y<-asS4(x,FALSE,complete);paste(isS4(y),identical(x,y),sep=",")},error=function(e)conditionMessage(e))
    emit("slots",complete,value,isS4(x))
  }
  setClass("ConversionEnvironment",contains="environment")
  e<-new.env(parent=emptyenv());e$marker<-41L
  x<-new("ConversionEnvironment",.xData=e);y<-asS4(x,FALSE)
  emit("environment",isS4(x),isS4(y),typeof(y),identical(y,e),y$marker)
})
