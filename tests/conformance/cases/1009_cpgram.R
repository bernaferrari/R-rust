cat(tryCatch(cpgram(cbind(1:5, 1:5)), error = function(e) conditionMessage(e)), "\n", sep = "")
cat(is.null(cpgram(1:5)), "\n", sep = "")
