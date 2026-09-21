function(x, value)
{
    y <- unclass(x)
    length(y) <- value
    .POSIXct(y, attr(x, "tzone"), oldClass(x))
}
