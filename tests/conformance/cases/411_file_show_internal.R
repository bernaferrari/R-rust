f <- tempfile()
writeLines("abc", f)
print(is.null(.Internal(file.show(f, "", "", TRUE, ""))))
print(file.exists(f))
x <- tryCatch(.Internal(file.show(character(), character(), "", FALSE, "")),
              error = function(e) conditionMessage(e))
print(x)
