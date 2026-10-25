# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(strtrim(matrix(c("雪a","éβ"),1,dimnames=list("r",c("a","b"))),2L))}),error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xe9, 0x9b, 0xaa)), as.raw(c(0xc3, 0xa9, 
0xce, 0xb2))), na = structure(c(FALSE, FALSE), dim = 1:2, dimnames = list(
    "r", c("a", "b"))), encoding = c("UTF-8", "UTF-8"), attributes = list(
    dim = 1:2, dimnames = list("r", c("a", "b"))), object = FALSE)))
