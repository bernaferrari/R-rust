print(tryCatch(quote(expr = 1), error = function(e) conditionMessage(e)))
print(tryCatch(quote(x = 1), error = function(e) conditionMessage(e)))
print(tryCatch(quote(a =), error = function(e) conditionMessage(e)))
