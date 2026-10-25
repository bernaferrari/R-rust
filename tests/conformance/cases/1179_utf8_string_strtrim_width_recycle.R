# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(strtrim(c("雪a","éβ","α雪"),c(1L,2L)))}),error=function(e) list(error=conditionMessage(e))),
list(error = "invalid 'width' argument")))
