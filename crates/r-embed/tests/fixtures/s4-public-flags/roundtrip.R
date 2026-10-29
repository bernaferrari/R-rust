local({
    values <- list(logical=c(TRUE,NA), integer=c(7L,8L), real=c(1.5,NA_real_),
        complex=c(1+2i,NA_complex_), character=c("café",NA_character_), raw=as.raw(c(0,255)),
        list=list(1L,"x"), expression=expression(1L,x+2L), pairlist=pairlist(a=1L),
        language=quote(f(x)), closure=function(x)x, compact=1:3, empty=integer(0),
        attributed=structure(c(1L,2L),class="example",note="attribute"))
    for (label in names(values)) {
        value <- values[[label]]
        for (marked in c(FALSE,TRUE)) {
            x <- if (marked) asS4(value) else value
            for (version in c(2L,3L)) {
                for (format in c("ascii","xdr","native")) {
                    y <- unserialize(serialize(x,NULL,version=version,ascii=format=="ascii",xdr=format!="native"))
                    cat(paste(label,marked,version,format,isS4(y),is.object(y),identical(x,y)),"\n",sep="")
                    y <- asS4(y,FALSE,complete=FALSE)
                    stopifnot(identical(isS4(x),marked),!isS4(value),!isS4(y))
                }
            }
        }
    }
})
