s<-enc2utf8(c("","a","\u00e9","\u00e9a","a\u00e9","\U0001f600a","a\U0001f600","\u00e9\U0001f600"))
for (mode in c("ere","perl","fixed")) for (bytes in c(FALSE,TRUE)) {
 z<-gregexpr("",s,perl=identical(mode,"perl"),fixed=identical(mode,"fixed"),useBytes=bytes)
 for (i in seq_along(z)) cat(mode,as.integer(bytes),i,paste(as.integer(z[[i]]),collapse=","),paste(attr(z[[i]],"match.length"),collapse=","),paste(attr(z[[i]],"index.type"),collapse=","),paste(attr(z[[i]],"useBytes"),collapse=","),"\n",sep="|")
}
