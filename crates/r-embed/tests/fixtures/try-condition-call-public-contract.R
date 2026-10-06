local({
    old <- options(useFancyQuotes = FALSE)
    on.exit(options(old))
    inner <- function() stop("inner")
    outer <- function() inner()
    nested <- withVisible(try(outer(), silent = TRUE))
    stopifnot(!nested$visible,
              identical(as.character(nested$value), "Error in inner() : inner\n"),
              identical(conditionCall(attr(nested$value, "condition")), quote(inner())))
    direct <- try(stop("direct"), silent = TRUE)
    stopifnot(identical(as.character(direct),
                       "Error in try(stop(\"direct\"), silent = TRUE) : direct\n"),
              identical(conditionCall(attr(direct, "condition")),
                        quote(doTryCatch(return(expr), name, parentenv, handler))))
    quiet <- try(stop("quiet", call. = FALSE), silent = TRUE)
    stopifnot(identical(as.character(quiet), "Error : quiet\n"),
              is.null(conditionCall(attr(quiet, "condition"))))
    primitive <- try(abs("a"), silent = TRUE)
    stopifnot(identical(as.character(primitive),
                       "Error in abs(\"a\") : non-numeric argument to mathematical function\n"),
              identical(conditionCall(attr(primitive, "condition")), quote(abs("a"))))
    condition <- errorCondition("classed", call = quote(target(x)), class = "custom")
    classed <- try(stop(condition), silent = TRUE)
    stopifnot(identical(as.character(classed), "Error in target(x) : classed\n"),
              identical(attr(classed, "condition"), condition))
    TRUE
})
