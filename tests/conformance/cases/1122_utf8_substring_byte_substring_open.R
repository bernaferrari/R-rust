# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";Encoding(x)<-"bytes";snap(substring(x,4L))}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xce, 0xb2))), na = FALSE, encoding = "bytes", 
    attributes = NULL)))
