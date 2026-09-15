cat(tryCatch(arima0.diag(), error = function(e) conditionMessage(e)), "\n", sep = "")
