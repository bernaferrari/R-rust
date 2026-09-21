function(target, current, check.environment = TRUE, ...)
{
    msg <- all.equal.language(target, current, ...)
    if (is.null(current))
        msg
    else if (check.environment) {
        ee <- identical(environment(target), environment(current))
        if (!ee) {
            Lt <- as.list(environment(target), all.names = TRUE)
            Lc <- as.list(environment(current), all.names = TRUE)
            ee <- if (identical(Lt, Lc)) TRUE else all.equal.list(Lt, Lc, ...)
        }
        if (isTRUE(msg))
            ee
        else c(msg, if (!isTRUE(ee)) ee)
    } else msg
}
