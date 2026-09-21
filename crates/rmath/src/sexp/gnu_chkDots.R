function(..., which.call = -1, allowed = character(0)) {
    if(nx <- ...length()) {
        if (!is.null(nms <- ...names())) {
            stopifnot(is.character(allowed))
            nms <- nms[!(nms %in% allowed)]
            nx <- length(nms)
            if (nx == 0L) return(invisible(NULL))
        }
        msg <- sprintf(ngettext(nx,
				 "In %s :\n extra argument %s will be disregarded",
				 "In %s :\n extra arguments %s will be disregarded"),
			paste(deparse(sys.call(which.call), control=c()), collapse="\n"),
			paste(sQuote(nms), collapse = ", "))
        warning(warningCondition(msg, class = c("chkDotsWarning", "simpleWarning")))
    }
    invisible(NULL)
}
