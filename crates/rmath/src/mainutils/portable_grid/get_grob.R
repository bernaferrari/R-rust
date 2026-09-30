function(gTree, gPath, strict = FALSE, grep = FALSE, global = FALSE) {
    if (!inherits(gTree, "gTree"))
        stop("it is only valid to get a child from a \"gTree\"")
    if (!is.logical(strict) || length(strict) != 1L ||
        !is.logical(grep) || !length(grep) ||
        !is.logical(global) || length(global) != 1L)
        stop("unsupported gPath matching options")
    if (anyNA(strict) || anyNA(grep) || anyNA(global)) stop("invalid matching option")
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
    use_grep <- rep(grep, length.out = length(names))
    name_matches <- function(pattern, value, use_grep) {
        if (is.null(value) || !nzchar(value)) return(FALSE)
        if (isTRUE(use_grep)) isTRUE(grepl(pattern, value)) else identical(as.character(pattern), value)
    }
    # One match stays a grob. Further matches become a gList, as growResult does.
    combine <- function(a, b) {
        items <- function(x) if (inherits(x, "gList")) x else list(x)
        structure(c(items(a), items(b)), class = "gList")
    }
    lookup <- function(tree, index) {
        if (!inherits(tree, "gTree")) return(NULL)
        found <- NULL
        pattern <- names[[index]]
        use <- use_grep[[index]]
        last <- index == length(names)
        for (child in tree$children) {
            child_name <- if (is.null(child$name)) "" else child$name
            hit <- NULL
            if (name_matches(pattern, child_name, use)) {
                hit <- if (last) child else lookup(child, index + 1L)
            } else if (!isTRUE(strict) && index == 1L) {
                hit <- lookup(child, 1L)
            }
            if (!is.null(hit)) {
                found <- if (is.null(found)) hit else combine(found, hit)
                if (!isTRUE(global)) break
            }
        }
        found
    }
    lookup(gTree, 1L)
}
