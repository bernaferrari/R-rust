function (e1, e2)
{
    ## need one scalar, one difftime.
    if(inherits(e2, "difftime"))
        stop("second argument of / cannot be a \"difftime\" object")
    .difftime(unclass(e1) / e2, attr(e1, "units"))
}
