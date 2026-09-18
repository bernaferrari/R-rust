function(filebase, fun, filter)
{
    glue <- function (..., sep = " ", collapse = NULL)
        paste(..., sep = sep, collapse = collapse)
    existsInFrame <- function (x, env) exists(x, envir = env, inherits = FALSE)
    mkenv <- function() new.env(hash = TRUE, parent = baseenv(), size = 29L)

    mapfile  <- glue(filebase, "rdx", sep = ".")
    datafile <- glue(filebase, "rdb", sep = ".")
    env <- mkenv()
    map <- readRDS(mapfile)
    vars <- names(map$variables)
    compressed <- map$compressed
    list2env(map$references, env)
    envenv <- mkenv()
    envhook <- function(n) {
        if (existsInFrame(n, envenv))
            envenv[[n]]
        else {
            e <- mkenv()
            envenv[[n]] <- e
            key <- env[[n]]
            ekey <- if (is.list(key)) key$eagerKey else key
            data <- lazyLoadDBfetch(ekey, datafile, compressed, envhook)
            parent.env(e) <- if (is.null(data$enclos)) emptyenv() else data$enclos
            list2env(data$bindings, e)
            if (! is.null(data$attributes))
                attributes(e) <- data$attributes
            if (! is.null(data$isS4) && data$isS4)
                asS4(e, TRUE, TRUE)
            if (is.list(key)) {
                expr <- quote(lazyLoadDBfetch(KEY, datafile, compressed, envhook))
                .Internal(makeLazy(names(key$lazyKeys), key$lazyKeys, expr,
                    parent.env(environment()), e))
            }
            if (! is.null(data$locked) && data$locked)
                lockEnvironment(e, FALSE)
            e
        }
    }
    if (!missing(filter)) {
        use <- filter(vars)
        vars <- vars[use]
        vals <- map$variables[use]
        use <- NULL
    } else
        vals <-  map$variables

    res <- fun(environment())

    map <- NULL
    vars <- NULL
    vals <- NULL
    rvars <- NULL
    mapfile <- NULL

    res
}

