# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(strtrim(c("雪a",NA_character_),2L))}),error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xe9, 0x9b, 0xaa)), as.raw(c(0x4e, 0x41
))), na = c(FALSE, TRUE), encoding = c("UTF-8", "unknown"), attributes = NULL, 
    object = FALSE)))
