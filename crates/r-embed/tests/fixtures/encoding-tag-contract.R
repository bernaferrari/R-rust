options(useFancyQuotes=FALSE)
u <- rawToChar(as.raw(c(195L,169L))); Encoding(u) <- "UTF-8"
l <- rawToChar(as.raw(233L)); Encoding(l) <- "latin1"
b <- rawToChar(as.raw(c(195L,169L))); Encoding(b) <- "bytes"
f <- rawToChar(as.raw(c(195L,169L,58L,37L,115L))); Encoding(f) <- "UTF-8"
run <- function(label, expr) {
  cat(label, "\n", sep="")
  tryCatch({
    z <- force(expr)
    if (is.character(z)) {
      print(Encoding(z))
      print(lapply(z, charToRaw))
    } else print(z)
  }, error=function(e) cat("ERROR: ", conditionMessage(e), "\n", sep=""))
}
run("input UTF8", u)
run("input Latin1", l)
run("input bytes", b)
run("sprintf UTF8 format UTF8 value", sprintf(f,u))
run("sprintf UTF8 format Latin1 value", sprintf(f,l))
run("paste UTF8 Latin1", paste(u,l))
run("paste Latin1 UTF8", paste(l,u))
run("paste bytes UTF8", paste0(b,u))
run("grep Latin1 pattern", grep(l,c(u,l,"z"),fixed=TRUE,value=TRUE))
run("regexpr UTF8 Latin1", regexpr(u,l))
run("sub UTF8 Latin1", sub(u,"x",l,fixed=TRUE))
run("pmatch mixed tags", pmatch(u,c(l,"z")))
n <- u; Encoding(n) <- "unknown"
run("input native UTF8 bytes", n)
run("input UTF8 after native copy", u)
run("paste native UTF8 ASCII", paste(n,"x"))
run("paste Latin1 Latin1", paste(l,l))
run("paste native UTF8 Latin1", paste(n,l))
run("paste UTF8 Latin1 collapse", paste(c(u,u),collapse=l))
run("paste bytes UTF8 collapse", paste(c(b,u),collapse=u))
run("sprintf native format Latin1 value", sprintf("%s",l))
run("sprintf byte value", sprintf("%s",b))

run("paste byte separator", paste(u,b,sep=b))
run("sprintf native format utf8 value",sprintf("%s",u))
run("paste byte collapse",paste(c(l,l),collapse=b))
