function(target, current, ..., check.class = TRUE)
{
    if (is.language(target))
        return(all.equal.language(target, current, ...))
    if (is.function(target))
        return(all.equal.function(target, current, ...))
    if (is.environment(target) || is.environment(current))
        return(all.equal.environment(target, current, ...))
    if (typeof(target) == "..." || typeof(current) == "...") {
        if (!(typeof(target) == "..." && typeof(current) == "..."))
            return(paste0("target is ", typeof(target), ", current is ", typeof(current)))
        if (identical(target, current))
            return(TRUE)
        lt <- length(target)
        lc <- length(current)
        if (lt != lc)
            return(paste0('"..."-typed": lengths (', lt, ", ", lc, ") differ"))
        "\"...\"-types of the same length, no names, but not identical"
    } else if (is.recursive(target))
        all.equal.list(target, current, ...)
    else {
        msg <- switch(mode(target),
                      integer = ,
                      complex = ,
                      numeric = all.equal.numeric(target, current, check.class = check.class, ...),
                      character = all.equal.character(target, current, check.class = check.class, ...),
                      logical = ,
                      raw = all.equal.raw(target, current, check.class = check.class, ...),
                      S4 = attr.all.equal(target, current, ...),
                      if (check.class && data.class(target) != data.class(current)) {
                          paste0("target is ", data.class(target), ", current is ",
                                 data.class(current))
                      } else NULL)
        if (is.null(msg)) TRUE else msg
    }
}
