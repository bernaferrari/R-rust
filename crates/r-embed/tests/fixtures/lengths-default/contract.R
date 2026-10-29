cases <- list(
 matrix=matrix(1:6,2,dimnames=list(c("r1","r2"),c("a","b","c"))),
 array=array(1:8,c(2,2,2),dimnames=list(c("a","b"),c("c","d"),c("e","f"))),
 list_matrix=matrix(list(NULL,1:3,TRUE,c("a","b")),2,dimnames=list(c("a","b"),c("c","d"))),
 empty_matrix=matrix(integer(),0,3,dimnames=list(NULL,c("a","b","c"))),
 names=c(a=1,b=2), named_list=list(a=NULL,b=1:3),
 expression=expression(1:3,NULL,x), null=NULL, logical=c(TRUE,NA),
 raw=as.raw(c(0,255)), complex=c(1+0i,NA_complex_),
 pairlist=pairlist(a=1), symbol=quote(x), closure=function() 1
)
encode <- function(x) if(is.null(x)) "NULL" else paste(x,collapse=",")
for(n in names(cases)) for(flag in c(TRUE,FALSE)) {
 ans <- tryCatch({
  y <- lengths(cases[[n]],use.names=flag)
  dn <- dimnames(y)
  paste(typeof(y),encode(y),encode(dim(y)),encode(names(y)),if(is.null(dn)) "NULL" else paste(vapply(dn,encode,""),collapse="/"),sep="|")
 },error=function(e) paste0("ERROR:",conditionMessage(e)))
 cat(n,flag,ans,sep="|",fill=TRUE)
}
flags <- list(NA,NULL,logical(),1,0,"TRUE",c(TRUE,FALSE))
for(i in seq_along(flags)) {
 ans <- tryCatch(paste(lengths(matrix(1:4,2),use.names=flags[[i]]),collapse=","),error=function(e) paste0("ERROR:",conditionMessage(e)))
 cat("flag",i,ans,sep="|",fill=TRUE)
}
x <- matrix(1:6,2,dimnames=list(c("a","b"),c("c","d","e")))
y <- lengths(x)
dim(y) <- c(3L,2L)
cat("result_mutation|",identical(dim(x),c(2L,3L)),"|",identical(dimnames(x),list(c("a","b"),c("c","d","e"))),"\n",sep="")
x <- matrix(1:4,2,dimnames=list(c("a","b"),c("c","d")))
y <- lengths(x)
dimnames(x) <- list(c("q","r"),c("s","t"))
cat("source_mutation|",identical(dimnames(y),list(c("a","b"),c("c","d"))),"\n",sep="")
cat("missing_flag|",identical(lengths(matrix(1:4,2),),matrix(1L,2,2)),"\n",sep="")
