# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(strtrim("e\u0301雪a",1L))}),error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0x65, 0xcc, 0x81))), na = FALSE, encoding = "UTF-8", 
    attributes = NULL, object = FALSE)))
