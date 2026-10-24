for (profile in c("unicode","bytes")) {
 bytes<-identical(profile,"bytes")
 p<-if(bytes) "(?<head>a)(?<tail>b)?(?<empty>)" else "(*UCP)(?<head>\u00e9|a)(?<tail>b)?(?<empty>)"
 x<-if(bytes) enc2utf8(c("\u00e9a","\u00e9ab","z",NA_character_)) else enc2utf8(c("\u00e9a","ab\u00e9","z",NA_character_))
 z<-gregexpr(p,x,perl=TRUE,useBytes=bytes)
 for(i in seq_along(z)) {
  a<-z[[i]];cs<-attr(a,"capture.start");cl<-attr(a,"capture.length");dn<-dimnames(cs)
  columns<-if(is.null(dn)) "" else paste(dn[[2L]],collapse=",")
  cat(profile,i,paste(as.integer(a),collapse=","),paste(attr(a,"match.length"),collapse=","),paste(as.integer(cs),collapse=","),paste(as.integer(cl),collapse=","),paste(dim(cs),collapse=","),paste(attr(a,"capture.names"),collapse=","),columns,paste(attr(a,"index.type"),collapse=","),paste(attr(a,"useBytes"),collapse=","),"\n",sep="|")
 }
}
