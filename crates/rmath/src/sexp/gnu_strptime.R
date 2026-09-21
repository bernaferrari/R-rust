function(x, format, tz = "") {
    r <-
    .Internal(strptime(if(is.character(x)) x
                       else if(is.object(x)) `names<-`(as.character(x), names(x))
                       else                  `storage.mode<-`(x, "character"),
                       format, tz))
    npI <- match(c("-Inf","Inf"), x, 0L)
    if(any(npI)) {
        if(npI[1L]) r[x == "-Inf"] <- as.POSIXlt.POSIXct(.POSIXct(-Inf))
        if(npI[2L]) r[x ==  "Inf"] <- as.POSIXlt.POSIXct(.POSIXct( Inf))
    }
    r
}
