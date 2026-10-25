# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({x<-"éaβ";z<-regmatches(x,regexpr("a",x,useBytes=TRUE));if(is.list(z)) lapply(z,snap) else snap(z)}), error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(0x61)), na = FALSE, encoding = "unknown", 
    attributes = NULL)))
