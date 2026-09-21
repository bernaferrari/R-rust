function(target, current, ..., check.class = TRUE)
{
    if (is.language(target))
        return(all.equal.language(target, current, ...))
    if (is.function(target)) {
        if (identical(target, current))
            return(TRUE)
        if (!is.function(current))
            return("current is not a function")
        return(all.equal.language(target, current, ...))
    }
    if (is.environment(target) || is.environment(current))
        return(all.equal(as.list(target), as.list(current), ...))
    if (is.recursive(target))
        return(all.equal.list(target, current, ...))
    msg <- switch(mode(target),
                  integer = ,
                  complex = ,
                  numeric = all.equal.numeric(target, current, check.class = check.class, ...),
                  character = all.equal.character(target, current, check.class = check.class, ...),
                  logical = ,
                  raw = all.equal.raw(target, current, check.class = check.class, ...),
                  if (check.class && data.class(target) != data.class(current)) {
                      paste0("target is ", data.class(target), ", current is ",
                             data.class(current))
                  } else NULL)
    if (is.null(msg)) TRUE else msg
}
