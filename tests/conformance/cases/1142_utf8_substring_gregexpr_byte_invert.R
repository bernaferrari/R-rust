# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";z<-regmatches(x,gregexpr("a",x,useBytes=TRUE),invert=TRUE);if(is.list(z)) lapply(z,snap) else snap(z)}), error=function(e) list(error=conditionMessage(e))),
list(list(raw = list(as.raw(c(0xc3, 0xa9)), as.raw(c(0xce, 0xb2
))), na = c(FALSE, FALSE), encoding = c("bytes", "bytes"), attributes = NULL))))
