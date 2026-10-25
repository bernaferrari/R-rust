# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";v<-"雪";Encoding(v)<-"bytes";substr(x,1L,3L)<-v;snap(x)}), error=function(e) list(error=conditionMessage(e))),
list(error = "translating strings with \"bytes\" encoding is not allowed")))
