emit <- function(label, expr) { cat(label,"|",tryCatch(paste(capture.output(dput(eval(expr))),collapse=" "),error=function(e)paste0("ERROR:",conditionMessage(e))),"\n",sep="") }
values <- list(1:3,c(TRUE,FALSE,NA),c(1.2,NA,Inf),as.raw(1:3),c(1+2i,NA,0+0i),structure(1:3,marker="é"))
for(i in seq_along(values)) for(mode in c("structure","attr")) {v<-values[[i]];x<-1:3;emit(paste(i,mode,sep=":"),quote({if(mode=="structure") x<-structure(x,names=v) else attr(x,"names")<-v;names(x)}))}
emit("unicode",quote(names(structure(1:3,names=c("é","日本","🙂")))))
emit("order",quote({x<-structure(1:3,first="é",names=c("a","b","c"),last="z");attr(x,"names")<-1:3;names(attributes(x))}))
emit("other",quote({x<-1:3;attr(x,"marker")<-1:3;attr(x,"marker")}))
emit("remove",quote({x<-structure(1:3,names=1:3);attr(x,"names")<-NULL;names(x)}))
emit("empty",quote(names(structure(integer(),names=integer()))))
emit("list",quote(names(structure(list(1,2,3),names=1:3))))
emit("expression",quote(names(structure(expression(1,2,3),names=1:3))))
emit("original",quote(names(structure(c("TRUE","FALSE",NA),names=1:3,class="unknown"))))
