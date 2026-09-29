function(x, incomparables = FALSE, ...)
    x[!duplicated(x, incomparables, ...)]
