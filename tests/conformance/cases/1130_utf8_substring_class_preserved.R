# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-structure(c("éaβ","γδ"),class="custom",note="source");snap(substr(x,1L,2L))}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xc3, 0xa9, 0x61)), as.raw(c(0xce, 0xb3, 
0xce, 0xb4))), na = c(FALSE, FALSE), encoding = c("UTF-8", "UTF-8"
), attributes = list(class = "custom", note = "source"))))
