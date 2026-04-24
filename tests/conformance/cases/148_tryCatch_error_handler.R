tryCatch(stop("boom"), error=function(e) "caught")
