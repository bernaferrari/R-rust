local({
  diagnostic <- function(raw, to="UTF-8", mark=TRUE) {
    tryCatch(iconv(list(raw),from="UTF-8",to=to,mark=mark),error=conditionMessage)
  }
  unicode <- as.raw(c(195,169,0,65))
  huge <- diagnostic(raw(10000))
  print(list(
    lengths=vapply(c(1L,100L,10000L,100000L),function(n) nchar(diagnostic(raw(n)),type="bytes"),integer(1)),
    huge_head=substr(huge,1L,40L),
    huge_tail=tail(charToRaw(huge),16L),
    utf8=diagnostic(unicode),
    utf8_unmarked=diagnostic(unicode,mark=FALSE),
    latin1=diagnostic(unicode,to="latin1"),
    latin1_unmarked=diagnostic(unicode,to="latin1",mark=FALSE),
    utf16=diagnostic(unicode,to="UTF-16LE"),
    literal_quote=diagnostic(as.raw(c(34,39,92,0))),
    controls=diagnostic(as.raw(c(7,8,9,10,11,12,13,127,0))),
    ascii=diagnostic(as.raw(c(65,0,66))),
    recovery=1L+1L
  ))
})
