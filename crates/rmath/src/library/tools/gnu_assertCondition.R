assertCondition <-
    function(expr, ...,
             .exprString = .deparseTrim(substitute(expr), cutoff = 30L),
             verbose = FALSE)
{
    getConds <- function(expr) {
	conds <- list()
        withCallingHandlers(
            tryCatch(expr, error = function(e) conds <<- c(conds, list(e))),
            warning = function(w) {
                conds <<- c(conds, list(w))
                tryInvokeRestart("muffleWarning")
            },
            condition = function(cond)
                conds <<- c(conds, list(cond)))
	conds
    }
    conds <- c(...)
    .Wanted <- if(length(conds)) paste(conds, collapse = " or ")
               else "any condition"
    res <- getConds(expr)
    if(length(res)) {
	if(is.null(conds)) {
            if(verbose)
                message("assertConditon: successfully caught a condition", domain = NA)
	    invisible(res)
        }
	else {
	    ii <- vapply(res,
                         function(cond) any(class(cond) %in% conds),
                         NA)
	    if(any(ii)) {
                if(verbose) {
                    found <- unique(unlist(lapply(res[ii], function(cond)
                        class(cond)[class(cond) %in% conds])))
                    message(sprintf("assertCondition: caught %s",
                                    paste(dQuote(found), collapse =", ")), domain = NA)
                }
		invisible(res)
            }
	    else {
                .got <- unique(unlist(lapply(res, function(obj) class(obj)[[1L]])))
		stop(gettextf("Got %s in evaluating %s; wanted %s",
			      paste(.got, collapse = ", "), .exprString, .Wanted),
                     domain = NA)
            }
	}
    }
    else
	stop(gettextf("Failed to get %s in evaluating %s",
		      .Wanted, .exprString), domain = NA)
}

assertError <- function(expr, classes = "error", verbose = FALSE) {
    d.expr <- .deparseTrim(substitute(expr), cutoff = 30L)
    tryCatch(res <- assertCondition(expr, classes, .exprString = d.expr),
             error = function(e)
                 stop(gettextf("Failed to get error in evaluating %s", d.expr),
                      call. = FALSE, domain = NA)
             )
    if(verbose) {
        error <- res[vapply(res,
                            function(cond) any(match(classes, class(cond), 0L) > 0L),
                            NA)]
        message(sprintf("Asserted error: %s", error[[1L]]$message),
                domain = NA)
    }
    invisible(res)
}

assertWarning <- function(expr, classes = "warning", verbose = FALSE) {
    d.expr <- .deparseTrim(substitute(expr), cutoff = 30L)
    res <- assertCondition(expr, classes, .exprString = d.expr)
    if(any(vapply(res,
                  function(cond) "error" %in% class(cond),
                  NA)))
        stop(gettextf("Got warning in evaluating %s, but also an error", d.expr),
             domain = NA)
    if(verbose) {
        warning <- res[vapply(res,
                              function(cond) any(match(classes, class(cond), 0L) > 0L),
                              NA)]
        message(sprintf("Asserted warning: %s", warning[[1L]]$message),
                domain = NA)
    }
    invisible(res)
}

.deparseTrim <- function(expr, cutoff = 30L) {
    res <- deparse(expr)
    if(length(res) > 1) {
        if(res[[1]] == "{") {
            exprs <- sub("^[ \t]*", "", res[c(-1L, -length(res))])
            res <- paste0("{", paste(exprs, collapse = "; "), "}")
        }
        else
            res <- paste(res[[1]], " ...")
    }
    if(nchar(res) > cutoff)
        paste(substr(res, 1, cutoff), " ...")
    else
        res
}
