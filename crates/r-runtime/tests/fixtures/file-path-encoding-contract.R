options(useFancyQuotes=FALSE)
u<-rawToChar(as.raw(c(0xc3,0xa9)));Encoding(u)<-"UTF-8"
l<-rawToChar(as.raw(0xe9));Encoding(l)<-"latin1"
b<-u;Encoding(b)<-"bytes"
n<-u;Encoding(n)<-"unknown"
run<-function(label,expr){cat(label,"\n",sep="");tryCatch({z<-force(expr);print(Encoding(z));print(lapply(z,charToRaw))},error=function(e)cat("ERROR: ",conditionMessage(e),"\n",sep=""))}
run("mixed",file.path(u,l))
run("bytes",file.path(b,u))
run("native",file.path(n,"x"))
run("Latin1",file.path(l,l))
run("numeric",file.path(1:2,TRUE))
run("zero",file.path(character(0),"x"))
run("NULL",file.path(NULL,"x"))
run("empty",file.path("","x",""))
run("fsep",file.path("a","b",fsep="."))
run("fsep bytes",file.path("a","b",fsep=b))
run("fsep Latin1",file.path("a","b",fsep=l))
run("fsep missing",file.path("a","b",fsep=NA_character_))
run("recycle",file.path(c("a","b"),1:3))
run("NA",file.path(c(NA,"x"),"y"))
run("internal",.Internal(file.path(list(u,l),"/")))
as.character.rport_path_contract <- function(x, ...) { gc(); l }
run("S3 collecting coercion",file.path(u,structure(1L,class="rport_path_contract")))
as.character.rport_path_bad_contract <- function(x, ...) 7L
run("S3 invalid coercion",file.path(u,structure(1L,class="rport_path_bad_contract")))
run("internal symbol",.Internal(file.path(list(as.name(u),l),"/")))
run("zero separator",file.path("x",fsep=character(0)))
run("empty dots before separator validation",file.path(fsep=NA_character_))
cat("FILEPATH_ENCODING_COMPLETE\n")
