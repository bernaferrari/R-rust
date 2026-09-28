summary.default <- function(object, ..., digits, quantile.type = 7,
                            character.method = c("default", "factor"),
                            polar = TRUE)
{
    if(is.factor(object))
	return(summary.factor(object, ...))
    else if(is.matrix(object)) {
	if(missing(digits))
	    return(summary.matrix(object,                  quantile.type=quantile.type, ...))
	else
	    return(summary.matrix(object, digits = digits, quantile.type=quantile.type, ...))
    }

    value <- if(is.logical(object)) { # scalar or array!
        tb <- table(object, exclude = NULL, useNA = "ifany") # incl. NAs
        if(!is.null(n <- dimnames(tb)[[1L]]) && any(iN <- is.na(n)))
            dimnames(tb)[[1L]][iN] <- "NAs"
        c(Mode = "logical", tb)
    } else if(is.numeric(object)) {
	nas <- is.na(object)
	object <- object[!nas]
	qq <- stats::quantile(object, names = FALSE, type = quantile.type)
        qq <- c(qq[1L:3L], mean(object), qq[4L:5L])
	if(!missing(digits)) qq <- signif(qq, digits)
	names(qq) <- c("Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max.")
	if(any(nas))
	    c(qq, "NAs" = sum(nas))
	else qq
    } else if(is.character(object) && !is.null(character.method)) {
        character.method <- match.arg(character.method)
        if(character.method == "factor")
            return(summary.factor(factor(object), ...))
        nas <- is.na(object)
        object <- object[!nas]
        ncs <- nchar(object, allowNA = TRUE) # NA if "bytes"-encoded
        nna <- sum(nas)
        c(Length    = length(nas),
          N.unique  = length(unique(object)), # NA excluded
          N.blank   = length(grep("^[ \t\r\n]*$", object, perl = TRUE)), # trimws()
          Min.nchar = if(length(ncs)) min(ncs) else NA_integer_,
          Max.nchar = if(length(ncs)) max(ncs) else NA_integer_,
          NAs       = if(nna > 0) nna)
    } else if(is.complex(object)) {
	nas <- is.na(object)
	object <- object[!nas]
	qop <- function(op)
	    stats::quantile(op(object), probs = c(0, 0.5, 1),
	                    names = FALSE, type = quantile.type)
	if(polar) { qq <- c(qop(Mod), qop(Arg)); nm <- c("Mod", "Arg") }
	else      { qq <- c(qop(Re ), qop(Im )); nm <- c("Re" , "Im" ) }
	if(!missing(digits)) qq <- signif(qq, digits)
	names(qq) <- paste0(c("Min.", "Median.", "Max."), rep(nm, each = 3L))
	if(any(nas))
	    c(qq, "NAs" = sum(nas))
	else qq
    } else if(is.recursive(object) && !is.language(object) &&
	      (n <- length(object))) { # do not allow long dims
	sumry <- array("", c(n, 3L), list(names(object),
                                          c("Length", "Class", "Mode")))
	ll <- numeric(n)
	for(i in 1L:n) {
	    ii <- object[[i]]
	    ll[i] <- length(ii)
	    cls <- oldClass(ii)
	    sumry[i, 2L] <- if(length(cls)) cls[1L] else "-none-"
	    sumry[i, 3L] <- mode(ii)
	}
	sumry[, 1L] <- format(as.integer(ll))
	sumry
    } else # very basic/all-purpose summary
        c(Length = length(object), Class = class(object), Mode = mode(object))
    class(value) <- c("summaryDefault", "table")
    value
}
