local({
 old <- options(digits=7, width=80)
 on.exit(options(old))
 probe <- function(name, f) {
  cat("case:", name, "\n")
  v <- tryCatch(withVisible(withCallingHandlers(f(), warning=function(w) {
   cat("warning:", conditionMessage(w), "\n")
   cat("warning call:", paste(deparse(conditionCall(w)),collapse=" "), "\n")
   invokeRestart("muffleWarning")
  })), error=function(e) e)
  if(inherits(v,"error")) {
   cat("error:", conditionMessage(v), "\n")
   cat("call:", paste(deparse(conditionCall(v)), collapse=" "), "\n")
  } else {
   cat("visible:", v$visible, "type:", typeof(v$value), "\n")
   print(v$value)
  }
 }
 stopifnot(is.function(base::prmatrix), !is.primitive(base::prmatrix),
           identical(environment(base::prmatrix), .BaseNamespaceEnv),
           identical(names(formals(prmatrix)), c("x","rowlab","collab","quote","right","na.print","...")))
 x <- matrix(c(1L, NA_integer_, 30L, 4L), 2,
             dimnames=list(c("r1","r two"),c("c1","c two")))
 original <- x
 probe("default integer", function() prmatrix(x))
 probe("explicit labels", function() prmatrix(x, rowlab=c("first","second"), collab=c("left","right")))
 probe("no labels", function() prmatrix(x, rowlab=NULL, collab=NULL))
 probe("custom NA", function() prmatrix(x, na.print="missing"))
 stopifnot(identical(x,original))
 for(v in list(matrix(c(TRUE,NA,FALSE,TRUE),2),
               matrix(c(pi,NA,Inf,-Inf),2),
               matrix(c("a",NA,"long","b"),2),
               matrix(c(1+2i,NA_complex_,3-4i,0+0i),2),
               matrix(as.raw(1:4),2),
               matrix(integer(),0,2), matrix(character(),2,0),
               1:3, data.frame(a=1:2,b=c("a","long")))) {
  probe(paste("input", typeof(v), paste(dim(v),collapse="x")), function() prmatrix(v))
 }
 chars <- matrix(c("a",NA,"long","b"),2)
 for(q in list(TRUE,FALSE,1,0,"TRUE","FALSE",NA,NULL,c(TRUE,FALSE)))
  probe(paste("quote",paste(q,collapse=",")), function() prmatrix(chars,quote=q))
 for(r in list(TRUE,FALSE,"TRUE",NA,NULL))
  probe(paste("right",paste(r,collapse=",")), function() prmatrix(chars,right=r))
 for(n in list(NULL,"?","",NA_character_,c("?","!"),1L,TRUE,character()))
  probe(paste("na.print",typeof(n),paste(n,collapse=",")), function() prmatrix(x,na.print=n))
 for(l in list(c("one","two"),"one",character(),c(1,2),c(TRUE,FALSE))) {
  probe(paste("rowlab",typeof(l),paste(l,collapse=",")), function() prmatrix(x,rowlab=l))
  probe(paste("collab",typeof(l),paste(l,collapse=",")), function() prmatrix(x,collab=l))
 }
 compiled <- compiler::cmpfun(prmatrix)
 probe("compiled explicit", function() compiled(x,rowlab=c("first","second"),collab=c("left","right"),na.print="?"))
 probe("complex custom NA",function() prmatrix(matrix(c(1+2i,NA_complex_),2),na.print="missing"))
 probe("matrix connection output",function() {
  con <- textConnection("captured","w",local=TRUE)
  sink(con)
  prmatrix(x,na.print="?")
  sink()
  close(con)
  cat(paste(captured,collapse="\n"),"\n",sep="")
  invisible(x)
 })
 for(f in list(function() array(),function() array(dim=c(1,1)),
               function() array(NULL),function() array(data=NULL,dim=c(0,1))))
  probe("array admission",f)
 probe("raw array print", function() print(array(as.raw(1:8),c(2,2,2))))
 for(v in list(NULL, complex(), c(1+2i,NA_complex_), as.raw(1:3),
               setNames(1:3,c("one","two","three")), array(1:8,c(2,2,2))))
  probe(paste("vector input",typeof(v),length(v)),function() prmatrix(v))
 for(rows in list(NULL,c("first","second"),c("1","2"))) {
  frame <- data.frame(a=1:2,b=c("a","long"),row.names=rows)
  probe(paste("frame rows",paste(rows,collapse=",")),function() prmatrix(frame))
  stopifnot(identical(dimnames(as.matrix(frame))[[1]], rows))
  formatted <- format(frame)
  probe(paste("formatted frame rows",paste(rows,collapse=",")),function() prmatrix(formatted))
  stopifnot(identical(dimnames(as.matrix(formatted))[[1]],row.names(frame)))
  for(force in list(NA,TRUE,FALSE))
   probe(paste("data.matrix rows",paste(rows,collapse=","),force),function() data.matrix(frame,rownames.force=force))
 }
 format.collecting <- function(x,...) { gc(); format(unclass(x),...) }
 collected <- data.frame(a=1:2,b=3:4)
 class(collected$a) <- c("collecting","integer")
 probe("formatted collecting columns",function() prmatrix(format(collected)))
 stopifnot(identical(x,original), identical(getOption("digits"),7L), identical(getOption("width"),80L))
})
