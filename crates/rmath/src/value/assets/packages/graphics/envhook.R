function(n) {
    if (existsInFrame(n, envenv)) 
        envenv[[n]]
    else {
        e <- mkenv()
        envenv[[n]] <- e
        key <- env[[n]]
        ekey <- if (is.list(key)) 
            key$eagerKey
        else key
        data <- lazyLoadDBfetch(ekey, datafile, compressed, envhook)
        parent.env(e) <- data$enclos %||% emptyenv()
        list2env(data$bindings, e)
        if (!is.null(data$attributes)) 
            attributes(e) <- data$attributes
        if (!is.null(data$isS4) && data$isS4) 
            .Internal(setS4Object(e, TRUE, TRUE))
        if (is.list(key)) {
            expr <- quote(lazyLoadDBfetch(KEY, datafile, compressed, 
                envhook))
            .Internal(makeLazy(names(key$lazyKeys), key$lazyKeys, 
                expr, parent.env(environment()), e))
        }
        if (!is.null(data$locked) && data$locked) 
            .Internal(lockEnvironment(e, FALSE))
        e
    }
}
