# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";substr(x,1L,NA_integer_)<-"雪";snap(x)}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0x4e, 0x41))), na = TRUE, encoding = "unknown", 
    attributes = NULL)))
