function(x, tz = "", ...)
{
    if((missing(tz) || is.null(tz)) &&
       !is.null(tzone <- attr(x, "tzone"))) tz <- tzone[1L]
    as.POSIXlt(x, tz)
}
