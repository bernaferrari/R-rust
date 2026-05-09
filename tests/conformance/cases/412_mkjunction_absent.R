print("mkjunction" %in% .Internal(builtins(TRUE)))
x <- tryCatch(.Internal(mkjunction("a", "b")), error = function(e) conditionMessage(e))
print(x)
