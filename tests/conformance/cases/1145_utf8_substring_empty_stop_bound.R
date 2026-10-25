# Pinned GNU R r90451; valid UTF-8 input and byte-coordinate semantics.
snap <- function(z) list(raw=lapply(as.vector(z),charToRaw), na=is.na(z),
                         encoding=Encoding(z), attributes=attributes(z))
print(identical(tryCatch(local({tryCatch(substr("éaβ",1L,integer()),error=function(e) conditionMessage(e))}), error=function(e) list(error=conditionMessage(e))),
"invalid substring arguments"))
