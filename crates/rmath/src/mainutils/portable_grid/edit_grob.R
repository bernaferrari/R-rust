function(grob, gPath = NULL, ..., strict = FALSE, grep = FALSE,
         global = FALSE, warn = TRUE) {
    specs <- list(...)
    if (!is.logical(strict) || length(strict) != 1L ||
        !is.logical(grep) || length(grep) != 1L ||
        !is.logical(global) || length(global) != 1L ||
        !is.logical(warn) || length(warn) != 1L)
        stop("unsupported gPath matching options")
    if (isTRUE(grep) || isTRUE(global))
        stop("grep and global gPath matching are not supported")
    if (is.na(strict) || is.na(grep) || is.na(global) || is.na(warn)) stop("invalid matching option")
    if (!inherits(grob, "grob")) stop("invalid grob")
    if (length(specs) && (is.null(names(specs)) || any(!nzchar(names(specs)))))
        stop("all grob edits must be named")
    edit_one <- function(value) {
        value <- structure(lapply(value, identity), names = names(value), class = class(value))
        for (field in names(specs)) {
            if (field %in% c("gp", "vp", "name")) {
                if (field == "gp" && !is.null(specs[[field]]) && !inherits(specs[[field]], "gpar"))
                    stop("invalid 'gp' value")
                if (field == "gp" && !is.null(value$gp) && !is.null(specs$gp)) {
                    gp <- structure(lapply(value$gp, identity), names=names(value$gp), class=class(value$gp))
                    for (parameter in names(specs$gp)) gp[[parameter]] <- specs$gp[[parameter]]
                    value$gp <- gp
                } else value[[field]] <- specs[[field]]
            } else if (!is.null(value$data) && field %in% names(value$data)) {
                stop("editing grob geometry is not supported yet")
            } else if (isTRUE(warn)) warning(sprintf("slot '%s' not found", field))
        }
        value
    }
    if (is.null(gPath))
        return(edit_one(grob))
    if (is.character(gPath)) {
        path_names <- unlist(strsplit(gPath, "::", fixed = TRUE), use.names = FALSE)
        if (!length(path_names) || any(!nzchar(path_names))) stop("invalid 'gPath'")
        gPath <- structure(list(path = if (length(path_names) > 1L)
                                    paste(path_names[-length(path_names)], collapse = "::") else NULL,
                                name = path_names[length(path_names)], n = length(path_names)),
                           class = c("gPath", "path"))
    }
    if (!inherits(grob, "gTree"))
        stop("it is only valid to edit a child of a \"gTree\"")
    if (!inherits(gPath, "gPath"))
        stop("invalid 'gPath'")
    names <- c(if (!is.null(gPath$path)) strsplit(gPath$path, "::", fixed = TRUE)[[1L]],
               gPath$name)
    edit_path <- function(tree, index) {
        if (!inherits(tree, "gTree")) return(NULL)
        children <- structure(lapply(tree$children, identity), names = names(tree$children), class = class(tree$children))
        name <- names[[index]]
        child <- children[[name]]
        changed <- FALSE
        if (!is.null(child)) {
            nested <- if (index == length(names)) edit_one(child) else edit_path(child, index + 1L)
            if (!is.null(nested)) {
                children[[name]] <- nested
                changed <- TRUE
            }
        }
        if (!changed && !isTRUE(strict) && index == 1L) {
            for (i in seq_along(children)) {
                nested <- edit_path(children[[i]], 1L)
                if (!is.null(nested)) {
                    children[[i]] <- nested
                    changed <- TRUE
                    break
                }
            }
        }
        if (!changed) return(NULL)
        names(children) <- vapply(children, function(child) if (is.null(child$name)) "" else child$name, character(1))
        tree <- structure(lapply(tree, identity), names = names(tree), class = class(tree))
        tree$children <- children
        tree
    }
    result <- edit_path(grob, 1L)
    if (is.null(result)) {
        if (isTRUE(warn)) warning(sprintf("'gPath' (%s) not found", paste(names, collapse = "::")))
        grob
    } else result
}
