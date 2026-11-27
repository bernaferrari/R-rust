args <- commandArgs(TRUE)
values <- list(logical=c(TRUE,NA), integer=c(7L,8L), real=c(1.5,NA_real_),
    complex=c(1+2i,NA_complex_), character=c("café",NA_character_), raw=as.raw(c(0,255)),
    list=list(1L,"x"), expression=expression(1L,x+2L), pairlist=pairlist(a=1L),
    language=quote(f(x)), closure=function(x)x, compact=1:3, empty=integer(0),
    attributed=structure(c(1L,2L),class="example",note="attribute"))
for (version in c(2L,3L)) {
    bytes <- serialize(lapply(values, asS4),NULL,version=version)
    writeBin(bytes,file.path(args[1],paste0("gnu-s4-v",version,".rds")))
}
