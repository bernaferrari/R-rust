# Pinned GNU R r90451; UTF-8 string-vector contract.
snap<-function(z) if(is.character(z)) list(raw=lapply(as.vector(z),charToRaw),na=is.na(z),encoding=Encoding(z),attributes=attributes(z),object=is.object(z)) else z
print(identical(tryCatch(local({snap({x<-c("é","雪");Encoding(x)<-"bytes";nchar(x,allowNA=TRUE)})}),error=function(e) list(error=conditionMessage(e))),
c(NA_integer_, NA_integer_)))
