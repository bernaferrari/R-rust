e <- new.env()
assign("x", 1, envir = e)
rm(list = "x", envir = e)
print(exists("x", e, inherits = FALSE))

assign("y", 2, envir = e)
lockEnvironment(e)
print(tryCatch({
  rm(list = "y", envir = e)
  "ok"
}, error = function(err) "locked environment"))
print(exists("y", e, inherits = FALSE))
