# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";z<-regmatches(x,regexec("(?<first>a)(?<last>β)",x,useBytes=TRUE,perl=TRUE));if(is.list(z)) lapply(z,snap) else snap(z)}), error=function(e) list(error=conditionMessage(e))),
list(list(raw = list(as.raw(c(0x61, 0xce, 0xb2)), as.raw(0x61), 
    as.raw(c(0xce, 0xb2))), na = c(FALSE, first = FALSE, last = FALSE
), encoding = c("bytes", "unknown", "bytes"), attributes = list(
    names = c("", "first", "last"))))))
