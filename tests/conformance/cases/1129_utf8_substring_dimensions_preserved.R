# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-matrix(c("éaβ","γδ"),nrow=1,dimnames=list("row",c("a","b")));snap(substr(x,1L,2L))}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xc3, 0xa9, 0x61)), as.raw(c(0xce, 0xb3, 
0xce, 0xb4))), na = structure(c(FALSE, FALSE), dim = 1:2, dimnames = list(
    "row", c("a", "b"))), encoding = c("UTF-8", "UTF-8"), attributes = list(
    dim = 1:2, dimnames = list("row", c("a", "b"))))))
