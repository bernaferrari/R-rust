local({
 old<-options(width=80)
 on.exit(options(old))
s<-enc2utf8(c("\u00e9","a","\u03a9","\u4e00","\u00df","\u0662","\u00a0"," ","2"))
for (pattern in c("^\\w+$","^\\d+$","^\\s+$","(?i)^k$","^.$","(*UCP)^\\w+$","(*UCP)^\\d+$","(*UCP)^\\s+$")) {
 cat(pattern,":",sep="")
 print(tryCatch(grepl(pattern,s,perl=TRUE),error=function(e)conditionMessage(e)))
}
})
