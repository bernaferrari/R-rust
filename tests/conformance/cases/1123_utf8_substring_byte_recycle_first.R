# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";Encoding(x)<-"bytes";snap(substring(x,c(1L,3L,4L),c(2L,3L,5L)))}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xc3, 0xa9)), as.raw(0x61), as.raw(c(0xce, 
0xb2))), na = c(FALSE, FALSE, FALSE), encoding = c("bytes", "unknown", 
"bytes"), attributes = NULL)))
