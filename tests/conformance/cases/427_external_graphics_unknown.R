msg <- tryCatch(.External.graphics("foo"), error = function(e) conditionMessage(e))
print(any(c(grepl("not in load table", msg), grepl("native extension code", msg))))
