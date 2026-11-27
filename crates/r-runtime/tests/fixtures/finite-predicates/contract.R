local({
  for (name in c("is.finite","is.infinite","is.nan")) {
    fun <- get(name)
    for (kind in c("null","logical","integer","real","complex","character","raw","list","expression","call","symbol","environment","closure","pairlist","builtin","special","object")) {
      x <- switch(kind,null=NULL,logical=c(TRUE,NA),integer=c(1L,NA_integer_),real=c(0,Inf,-Inf,NA_real_,NaN),complex=c(1+2i,complex(real=Inf,imaginary=NaN),NA_complex_),character=c("x",NA_character_),raw=as.raw(c(0,255)),list=list(1L),expression=expression(x),call=quote(f(x)),symbol=quote(x),environment=new.env(parent=emptyenv()),closure=function(x)x,pairlist=pairlist(a=1),builtin=sum,special=get("if"),object=.OBJSXP())
      result <- tryCatch({ y <- fun(x); paste(typeof(y),length(y),paste(y,collapse=","),sep="|") },error=function(e)paste("ERROR",conditionMessage(e),sep="|"))
      cat(name,kind,result,"\n",sep="|")
    }
  }
  x <- structure(c(1,Inf,NA_real_,NaN),names=c("one","inf","na","nan"),note="drop")
  for (name in c("is.finite","is.infinite","is.nan")) {
    y <- get(name)(x)
    cat(name,"names",paste(names(y),collapse=","),is.null(attr(y,"note")),identical(x,structure(c(1,Inf,NA_real_,NaN),names=c("one","inf","na","nan"),note="drop")),"\n",sep="|")
  }
})
