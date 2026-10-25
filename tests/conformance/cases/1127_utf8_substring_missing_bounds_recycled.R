# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-c("éaβ","γδ","雪");snap(substr(x,c(1L,NA_integer_),c(1L,2L,NA_integer_)))}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xc3, 0xa9)), as.raw(c(0x4e, 0x41)), 
    as.raw(c(0x4e, 0x41))), na = c(FALSE, TRUE, TRUE), encoding = c("UTF-8", 
"unknown", "unknown"), attributes = NULL)))
