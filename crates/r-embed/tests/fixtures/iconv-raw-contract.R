local({
  probe <- function(expr) tryCatch(expr, error=function(e) paste0("ERROR: ", conditionMessage(e)))
  bytes <- function(x) if (is.character(x)) lapply(x, function(s) if (is.na(s)) NULL else charToRaw(s)) else x
  u <- intToUtf8(c(65L,233L,8364L,128578L))
  latin <- rawToChar(as.raw(c(99,97,102,233))); Encoding(latin) <- "latin1"
  bad <- rawToChar(as.raw(c(65,255,66)))
  inputs <- list(as.raw(c(65,0,233,0)),NULL,raw())
  print(list(
    unknown_from=probe(iconv("x",from="not-an-encoding",to="UTF-8")),
    unknown_to=probe(iconv("x",from="UTF-8",to="not-an-encoding")),
    unknown_empty=probe(iconv(character(),from="not-an-encoding",to="UTF-8")),
    unknown_na=probe(iconv(NA_character_,from="not-an-encoding",to="UTF-8")),
    raw=probe(iconv(c("a",NA_character_,""),to="UTF-8",toRaw=TRUE)),
    raw_list=probe(bytes(probe(iconv(inputs,from="UTF-16LE",to="UTF-8")))),
    utf16le=probe(iconv(u,from="UTF-8",to="UTF-16LE",toRaw=TRUE)),
    utf16be=probe(iconv(u,from="UTF-8",to="UTF-16BE",toRaw=TRUE)),
    utf32le=probe(iconv(u,from="UTF-8",to="UTF-32LE",toRaw=TRUE)),
    utf32be=probe(iconv(u,from="UTF-8",to="UTF-32BE",toRaw=TRUE)),
    roundtrips=probe(lapply(c("UTF-16LE","UTF-16BE","UTF-32LE","UTF-32BE"),function(enc) {
      raw <- iconv(u,from="UTF-8",to=enc,toRaw=TRUE)
      bytes(iconv(raw,from=enc,to="UTF-8"))
    })),
    invalid16=probe(iconv(list(as.raw(c(0x41,0,0,0xd8,0x42,0))),from="UTF-16LE",to="UTF-8",sub="byte")),
    invalid32=probe(iconv(list(as.raw(c(0,0,0x11,0))),from="UTF-32LE",to="UTF-8",sub="byte")),
    invalidutf8=probe(iconv(bad,from="UTF-8",to="UTF-8",sub="byte")),
    invaliddrop=probe(iconv(bad,from="UTF-8",to="UTF-8",sub="")),
    invalidna=probe(iconv(bad,from="UTF-8",to="UTF-8",toRaw=TRUE)),
    byte_sub=probe(iconv(intToUtf8(233),from="UTF-8",to="ASCII",sub="byte")),
    literal_sub=probe(iconv(intToUtf8(233),from="UTF-8",to="ASCII",sub="?")),
    unicode_sub=probe(iconv(u,from="UTF-8",to="ASCII",sub="Unicode")),
    c99_sub=probe(iconv(u,from="UTF-8",to="ASCII",sub="c99")),
    latin_byte=probe(iconv(latin,from="latin1",to="ASCII",sub="byte")),
    latin_default=probe(bytes(iconv(latin,to="UTF-8"))),
    raw_attrs=probe(attributes(iconv(structure(c("a",NA),names=c("a","b"),hello=4L),toRaw=TRUE))),
    raw_input_attrs=probe(attributes(iconv(structure(list(as.raw(65)),names="n",hello=5L),from="ASCII",to="UTF-8"))),
    invalidmark=probe(iconv("x",mark=NA)),
    invalidraw=probe(iconv("x",toRaw=NA)),
    invalidfrom=probe(iconv("x",from=c("ASCII","UTF-8"))),
    invalidto=probe(iconv("x",to=character())),
    invalidsub=probe(iconv("x",sub=c("a","b"))),
    invalidlist=probe(iconv(list(1L),from="UTF-8",to="UTF-8")),
    nul=probe(iconv(list(as.raw(c(65,0,66))),from="UTF-8",to="UTF-8")),
    nulraw=probe(iconv(list(as.raw(c(65,0,66))),from="UTF-8",to="UTF-8",toRaw=TRUE)),
    unicode_nul=probe(iconv("a",to="UTF-16LE")),
    marks=probe(Encoding(iconv(c(latin,latin),to="UTF-8"))),
    unmarked=probe(Encoding(iconv(latin,to="UTF-8",mark=FALSE))
)
  ))
})
