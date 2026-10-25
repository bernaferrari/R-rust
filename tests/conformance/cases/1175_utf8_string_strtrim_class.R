# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(strtrim(structure("雪a",class="custom",note="source"),2L))}),error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xe9, 0x9b, 0xaa))), na = FALSE, encoding = "UTF-8", 
    attributes = list(class = "custom", note = "source"), object = TRUE)))
