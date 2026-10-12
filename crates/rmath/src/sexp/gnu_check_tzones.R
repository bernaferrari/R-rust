function(...)
{
    tz1 <- function(x) attr(x, "tzone")[1L] %||% ""
    tzs <- unique(vapply(list(...), tz1, ""))
    tzs <- tzs[nzchar(tzs)]
    if(length(tzs) > 1L)
        warning("'tzone' attributes are inconsistent")
    if(length(tzs)) tzs[1L] else NULL
}
