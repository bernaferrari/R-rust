cases <- list(
 atomic_missing=list(list(c(1, NA_real_))),
 atomic_clean=list(list(c(1,2))),
 singleton=list(list(NA_integer_)),
 logical=list(list(c(FALSE,NA))),
 complex=list(list(c(1+0i,NA_complex_))),
 character=list(list(c("ok",NA_character_))),
 raw=list(list(as.raw(c(0,255)))),
 empty=list(list(NULL,list())),
 pairlist=list(pairlist(clean=1:2, nested=list(c(3,NA_real_)))),
 pairlist_outer=pairlist(first=1:2, second=list(list(c(1,NaN)))),
 siblings=list(list(1:3),list(list(c(1,NA_real_)))),
 short_circuit=list(list(c(1,NA_real_)),list(quote(x)))
)
for (n in names(cases)) for (recursive in c(FALSE,TRUE)) {
 result <- tryCatch(as.character(anyNA(cases[[n]],recursive=recursive)), error=function(e) paste0("ERROR:",conditionMessage(e)))
 cat(n,recursive,result,sep="|",fill=TRUE)
}
is.na.nested_probe <- function(x) { gc(); c(FALSE,TRUE) }
cat("class|",anyNA(list(list(structure(1,class="nested_probe"))),recursive=TRUE),"\n",sep="")
rm(is.na.nested_probe)
x <- list(a=list(b=c(mark=NA_real_)))
y <- anyNA(x,recursive=TRUE)
cat("unchanged|",identical(x,list(a=list(b=c(mark=NA_real_)))) ,"|",typeof(y),"|",length(y),"\n",sep="")
flags <- list(1,0,"TRUE",NA,logical(),c(TRUE,FALSE))
for (i in seq_along(flags)) {
 result <- tryCatch(as.character(anyNA(list(list(c(1,NA_real_))),recursive=flags[[i]])),error=function(e) paste0("ERROR:",conditionMessage(e)))
 cat("flag",i,result,sep="|",fill=TRUE)
}
cat("atomic_ignores_flag|",anyNA(c(1,NA_real_),recursive=NA),"\n",sep="")
anyNA.top_probe <- function(x,recursive=FALSE) { gc(); TRUE }
cat("top_dispatch|",anyNA(structure(1,class="top_probe")),"\n",sep="")
rm(anyNA.top_probe)
is.na.na_probe <- function(x) { gc(); NA }
cat("na_fallback|",anyNA(structure(1,class="na_probe")),"|",anyNA(list(list(structure(1,class="na_probe"))),recursive=TRUE),"\n",sep="")
rm(is.na.na_probe)
deep <- NA_real_
for (i in seq_len(512L)) deep <- list(deep)
cat("deep_missing|",anyNA(deep,recursive=TRUE),"\n",sep="")
deep <- 1
for (i in seq_len(512L)) deep <- list(deep)
cat("deep_clean|",anyNA(deep,recursive=TRUE),"\n",sep="")
shared <- list(c(1,2))
cat("shared_dag|",anyNA(list(shared,shared),recursive=TRUE),"\n",sep="")
