e <- new.env()
assign("x", 42, envir = e)
print(exists("x", envir = e))
print(exists("x", envir = emptyenv()))
