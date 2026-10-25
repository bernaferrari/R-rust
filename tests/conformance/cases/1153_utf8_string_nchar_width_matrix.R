# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(nchar(matrix(c("é","雪a",NA_character_,""),2,dimnames=list(c("r","s"),c("a","b"))),type="width"))}),error=function(e) list(error=conditionMessage(e))),
structure(c(1L, 3L, 2L, 0L), dim = c(2L, 2L), dimnames = list(
    c("r", "s"), c("a", "b")))))
