tryCatch(stop("boom"), error=function(e) conditionMessage(e))
