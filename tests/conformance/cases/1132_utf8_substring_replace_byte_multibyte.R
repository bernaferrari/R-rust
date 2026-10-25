# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";Encoding(x)<-"bytes";v<-"雪";Encoding(v)<-"bytes";substr(x,1L,3L)<-v;snap(x)}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xe9, 0x9b, 0xaa, 0xce, 0xb2))), na = FALSE, 
    encoding = "bytes", attributes = NULL)))
