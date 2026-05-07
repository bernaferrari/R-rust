print(tryCatch(quote(), error = function(e) conditionMessage(e)))
print(tryCatch(quote(,), error = function(e) conditionMessage(e)))
