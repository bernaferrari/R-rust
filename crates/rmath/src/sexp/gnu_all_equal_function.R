function(target, current, check.environment = TRUE, ...)
{
    msg <- all.equal.language(target, current, ...)
    if (is.null(current))
        msg
    else if (check.environment) {
        ee <- identical(environment(target), environment(current))
        if (!ee)
            ee <- all.equal.environment(environment(target),
                                        environment(current), ...)
        if (isTRUE(msg))
            ee
        else c(msg, if (!isTRUE(ee)) ee)
    } else msg
}
