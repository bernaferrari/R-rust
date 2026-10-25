# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(nchar(c(un="é",deux="雪a",trois=NA_character_),type="width"))}),error=function(e) list(error=conditionMessage(e))),
c(un = 1L, deux = 3L, trois = 2L)))
