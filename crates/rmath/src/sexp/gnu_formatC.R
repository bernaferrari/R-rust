function (x, digits = NULL, width = NULL,
	     format = NULL, flag = "", mode = NULL,
	     big.mark = "", big.interval = 3L,
	     small.mark = "", small.interval = 5L,
             decimal.mark = getOption("OutDec"),
             preserve.width = "individual", zero.print = NULL,
	     replace.zero = TRUE,
             drop0trailing = FALSE)
{
    if(is.object(x)) {
	if(!(is.atomic(x) || inherits(x, "vector")))
	    warning("class of 'x' was discarded")
        x <- unclass(x)
    }
    ## sanity check for flags added 2.1.0
    flag <- as.character(flag)
    if(length(flag) != 1)
	stop(gettextf("'%s' must be a character string", "flag"), domain = NA)
    nf <- strsplit(flag, "")[[1L]]
    if(!all(nf %in% c("0", "+", "-", " ", "#", "'", "I")))
	stop("'flag' should contain only characters from [0+- #'I]")

    format.char <- function (x, width, flag)
    {
	if(is.null(width)) width <- 0L
	else if(width < 0L) { flag <- "-"; width <- -width }
	format.default(x, width=width,
		       justify = if(flag=="-") "left" else "right")
    }

    if (!(n <- length(x))) return(character())
    if (is.null(mode))	  mode <- storage.mode(x)
    else if (any(mode == c("double", "real", "integer")))  {
      ## for .C call later on
	if(mode == "real") mode <- "double"
	storage.mode(x) <- mode
    }
    else if (mode != "character")
        stop("'mode' must be \"double\" (\"real\"), \"integer\" or \"character\"")
    if (mode == "character" || (!is.null(format) && format == "s")) {
	if (mode != "character") {
	    warning('coercing argument to "character" for format="s"')
	    x <- as.character(x)
	}
	return(format.char(x, width=width, flag=flag))
    }
    if (missing(format) || is.null(format))
	format <- if (mode == "integer") "d" else "g"
    else {
	if (any(format == c("f", "e", "E", "g", "G", "fg"))) {
	    if (mode == "integer") mode <- storage.mode(x) <- "double"
	}
	else if (format == "d") {
	    if (mode != "integer") mode <- storage.mode(x) <- "integer"
	}
	else stop('\'format\' must be one of {"f","e","E","g","G", "fg", "s"}')
    }
    some.special <- !all(Ok <- is.finite(x))
    if (some.special) {
	rQ <- as.character(x[!Ok])
	rQ[is.na(rQ)] <- "NA"
	x[!Ok] <- as.vector(0, mode = mode)
    }
    if(is.null(width) && is.null(digits))
	width <- 1L
    if (is.null(digits))
	digits <- if (mode == "integer") 2L else 4L
    else if(digits < 0L)
	digits <- 6L
    else {
	maxDigits <- if(format != "f") 50L else
	    ceiling(-(.Machine$double.neg.ulp.digits + .Machine$double.min.exp) / log2(10))
	if (digits > maxDigits) {
            warning(gettextf("'digits' reduced to %d", maxDigits), domain = NA)
            digits <- maxDigits
	}
    }
    if(is.null(width))	width <- digits + 1L
    else if (width == 0L) width <- digits
    i.strlen <-
	pmax(abs(as.integer(width)),
	     if(format == "fg" || format == "f") {
		 xEx <- as.integer(floor(log10(abs(x + (x==0)))))
		 as.integer(x < 0 | flag!="") + digits +
		     if(format == "f") {
			 2L + pmax(xEx, 0L)
		     } else {# format == "fg"
			 1L + pmax(xEx, digits, digits + (-xEx) + 1L) +
			     length(nf) # == nchar(flag, "b")
		     }
	     } else # format == "g" or "e":
		 rep.int(digits + 8L, n)
	     )
    if(digits > 0 && any(nf == "#"))
	digits <- -digits # C-code will notice "do not drop trailing zeros"

    attr(x, "Csingle") <- NULL	# avoid interpreting as.single
    r <- .Internal(formatC(x, as.character(mode), width, digits,
			   as.character(format), flag, i.strlen))
    if (some.special) r[!Ok] <- format.char(rQ, width = width, flag = flag)

    if(nzchar(big.mark) || nzchar(small.mark) || decimal.mark != "." ||
       !is.null(zero.print) || drop0trailing)
	r <- prettyNum(r, big.mark = big.mark, big.interval = big.interval,
		       small.mark = small.mark, small.interval = small.interval,
		       decimal.mark = decimal.mark, input.d.mark = ".",
		       preserve.width = preserve.width, zero.print = zero.print,
		       replace.zero = replace.zero,
		       drop0trailing = drop0trailing, is.cmplx = FALSE)

    if (!is.null(x.atr <- attributes(x)))
	attributes(r) <- x.atr
    r
}
