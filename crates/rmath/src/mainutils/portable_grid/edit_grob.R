function(grob, gPath = NULL, ..., strict = FALSE, grep = FALSE,
         global = FALSE, warn = TRUE) {
    caller <- parent.frame()
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
        edit_method <- NULL
        for (class_name in class(value)) {
            candidate <- do.call("getS3method",
                list("editDetails", class_name, optional = TRUE), envir = caller)
            if (is.function(candidate)) {
                edit_method <- candidate
                break
            }
        }
        # Primitive grobs keep their drawing arguments in `data`.  GNU grid's
        # editDetails methods validate the complete geometry after applying an
        # edit, so validate the candidate values before returning the copy.
        primitive <- if (!is.null(value$primitive)) value$primitive else NULL
        geometry <- if (is.null(primitive)) character() else switch(primitive,
            rect = c("x", "y", "width", "height", "just", "hjust", "vjust"),
            circle = c("x", "y", "r"),
            lines = c("x", "y"),
            segments = c("x0", "y0", "x1", "y1"),
            polygon = c("x", "y", "id", "id.lengths"),
            points = c("x", "y", "size", "pch"),
            text = c("label", "x", "y", "just", "hjust", "vjust", "rot", "check.overlap"),
            character())
        data <- if (is.null(value$data)) list() else
            structure(lapply(value$data, identity), names = names(value$data), class = class(value$data))
        for (field in names(specs)) {
            if (field %in% c("gp", "vp", "name")) {
                if (field == "gp" && !is.null(specs[[field]]) && !inherits(specs[[field]], "gpar"))
                    stop("invalid 'gp' value")
                if (field == "gp" && !is.null(value$gp) && !is.null(specs$gp)) {
                    gp <- structure(lapply(value$gp, identity), names=names(value$gp), class=class(value$gp))
                    for (parameter in names(specs$gp)) gp[[parameter]] <- specs$gp[[parameter]]
                    value$gp <- gp
                } else value[[field]] <- specs[[field]]
            } else if (field %in% geometry && field %in% names(data)) {
                data[[field]] <- specs[[field]]
            } else if (field %in% names(value)) {
                value[[field]] <- specs[[field]]
            } else if (isTRUE(warn)) warning(sprintf("slot '%s' not found", field))
        }
        if (!is.null(edit_method)) {
            if (!is.null(primitive)) value$data <- data
            return(do.call(editDetails, list(value, specs), envir = caller))
        }
        if (length(geometry)) {
            unit_fields <- if (is.null(primitive)) character() else switch(primitive,
                rect = c("x", "y", "width", "height"),
                circle = c("x", "y", "r"),
                lines = c("x", "y"),
                segments = c("x0", "y0", "x1", "y1"),
                polygon = c("x", "y"),
                points = c("x", "y", "size"),
                text = c("x", "y"),
                character())
            if (length(unit_fields) && any(!vapply(unit_fields,
                    function(field) is.unit(data[[field]]), logical(1)))) {
                stop(sprintf("'%s' must be units", paste(unit_fields, collapse = "', '")))
            }
            if (primitive == "polygon") {
                n <- length(data$x)
                if (!is.null(data$id) && !is.null(data$id.lengths))
                    stop("it is invalid to specify both 'id' and 'id.lengths'")
                if (length(data$y) != n)
                    stop("'x' and 'y' must be units and have the same length")
                if (!is.null(data$id) && length(data$id) != n)
                    stop("'x' and 'y' and 'id' must all be same length")
                if (!is.null(data$id.lengths) && sum(data$id.lengths) != n)
                    stop("'x' and 'y' and 'id.lengths' must specify same overall length")
                if (!is.null(data$id)) data$id <- as.integer(data$id)
                if (!is.null(data$id.lengths)) data$id.lengths <- as.integer(data$id.lengths)
            }
            if (primitive == "points" && length(data$x) != length(data$y))
                stop("'x' and 'y' must be units and have the same length")
            if (primitive == "text") {
                if (!is.language(data$label)) data$label <- as.character(data$label)
                data$rot <- as.numeric(data$rot)
                if (!length(data$rot) || !all(is.finite(data$rot))) stop("invalid 'rot' value")
                data$check.overlap <- as.logical(data$check.overlap)
            }
        }
        if (length(geometry) && !is.null(value$data)) value$data <- data
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
