function(gTree, gPath, strict = FALSE, grep = FALSE, global = FALSE) {
    if (!inherits(gTree, "gTree"))
        stop("it is only valid to get a child from a \"gTree\"")
    if (!is.logical(strict) || length(strict) != 1L ||
        !is.logical(grep) || length(grep) != 1L ||
        !is.logical(global) || length(global) != 1L)
        stop("unsupported gPath matching options")
    if (is.na(strict) || is.na(grep) || is.na(global)) stop("invalid matching option")
    if (isTRUE(grep) || isTRUE(global))
        stop("grep and global gPath matching are not supported")
    if (is.character(gPath)) {
        path_names <- unlist(strsplit(gPath, "::", fixed = TRUE), use.names = FALSE)
        if (!length(path_names) || any(!nzchar(path_names))) stop("invalid 'gPath'")
        gPath <- structure(list(path = if (length(path_names) > 1L)
                                    paste(path_names[-length(path_names)], collapse = "::") else NULL,
                                name = path_names[length(path_names)], n = length(path_names)),
                           class = c("gPath", "path"))
    }
    if (!inherits(gPath, "gPath"))
        stop("invalid 'gPath'")
    names <- c(if (!is.null(gPath$path)) strsplit(gPath$path, "::", fixed = TRUE)[[1L]],
               gPath$name)
    lookup <- function(tree, index) {
        if (!inherits(tree, "gTree")) return(NULL)
        child <- tree$children[[names[[index]]]]
        if (!is.null(child)) {
            if (index == length(names)) return(child)
            found <- lookup(child, index + 1L)
            if (!is.null(found)) return(found)
        }
        if (!isTRUE(strict) && index == 1L) {
            for (child in tree$children) {
                found <- lookup(child, 1L)
                if (!is.null(found)) return(found)
            }
        }
        NULL
    }
    lookup(gTree, 1L)
}
