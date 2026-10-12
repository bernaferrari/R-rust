function(e1, e2)
{
    coerceTimeUnit <- function(x) # coerce to secs
        switch(attr(x, "units"),
               secs = x, mins = 60*x, hours = 60*60*x,
               days = 60*60*24*x, weeks = 60*60*24*7*x)
    if (nargs() == 1L) {
        switch(.Generic, "+" = {}, "-" = {e1[] <- -unclass(e1)},
               stop(gettextf("unary '%s' not defined for \"difftime\" objects",
                             .Generic), domain = NA, call. = FALSE)
               )
        return(e1)
    }
    boolean <- switch(.Generic, "<" = , ">" = , "==" = ,
                      "!=" = , "<=" = , ">=" = TRUE, FALSE)
    if (boolean) {
        ## assume user knows what he/she is doing if not both difftime
        if(inherits(e1, "difftime") && inherits(e2, "difftime")) {
            e1 <- coerceTimeUnit(e1)
            e2 <- coerceTimeUnit(e2)
        }
        NextMethod(.Generic)
    } else if(.Generic == "+" || .Generic == "-") {
        if(inherits(e1, "difftime") && !inherits(e2, "difftime"))
            return(.difftime(NextMethod(.Generic),
                             units = attr(e1, "units")))
        if(!inherits(e1, "difftime") && inherits(e2, "difftime"))
            return(.difftime(NextMethod(.Generic),
                             units = attr(e2, "units")))
        u1 <- attr(e1, "units")
        if(attr(e2, "units") == u1) {
            .difftime(NextMethod(.Generic), units = u1)
        } else {
            e1 <- coerceTimeUnit(e1)
            e2 <- coerceTimeUnit(e2)
            .difftime(NextMethod(.Generic), units = "secs")
        }
    } else {
        ## '*' is covered by a specific method
        stop(gettextf("'%s' not defined for \"difftime\" objects", .Generic),
             domain = NA)
    }
}
