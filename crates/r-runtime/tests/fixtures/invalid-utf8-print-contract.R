local({
  marked <- rawToChar(as.raw(c(65,195,169,255,244,144,128,128,233)))
  Encoding(marked) <- "UTF-8"
  native <- rawToChar(as.raw(c(65,255,66)))
  Encoding(native) <- "unknown"
  print(list(
    marked=c("ok",marked,intToUtf8(233)),
    native=c("ok",native,"end"),
    matrix=matrix(c("x",marked,"a",intToUtf8(233)),nrow=2),
    noquote=noquote(c(marked,"x")),
    encoded=encodeString(c("x",marked),width=NA,quote='"',justify="right"),
    unicode=c(intToUtf8(c(65,233,128578)),"ASCII")
  ))
})
