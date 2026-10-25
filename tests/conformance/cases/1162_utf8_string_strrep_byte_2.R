# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap(strrep({x<-"é";Encoding(x)<-"bytes";x},2L))}),error=function(e) list(error=conditionMessage(e))),
list(raw = list(as.raw(c(0xc3, 0xa9, 0xc3, 0xa9))), na = FALSE, 
    encoding = "bytes", attributes = NULL, object = FALSE)))
