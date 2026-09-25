function (what, where = FALSE, ignore.case = TRUE, mode = "any")
{
    stopifnot(is.character(what))
    x <- character(0L)
    check.mode <- mode != "any"
    for (i in seq_along(search())) {
	li <- grep(what, ls(pos = i, all.names = TRUE),
		   ignore.case = ignore.case, value = TRUE)
        li <- grep("^[.](__|C_|F_)", li, invert = TRUE, value = TRUE)
	if(length(li)) {
	    if(check.mode)
		li <- li[vapply(li, exists, NA, where = i,
				mode = mode, inherits = FALSE)]
	    x <- c(x, if(where) structure(li, names = rep.int(i, length(li))) else li)
	}
    }
    sort(x)
}
