env <- new.env()
assign("x", 1, envir = env)

print(environmentIsLocked(env))
lockBinding("x", env)
print(bindingIsLocked("x", env))
print(tryCatch({ assign("x", 2, envir = env); FALSE }, error = function(e) TRUE))

unlockBinding("x", env)
assign("x", 2, envir = env)
print(env$x)

lockEnvironment(env, bindings = TRUE)
print(environmentIsLocked(env))
print(bindingIsLocked("x", env))
print(tryCatch({ assign("y", 3, envir = env); FALSE }, error = function(e) TRUE))
print(tryCatch({ assign("x", 4, envir = env); FALSE }, error = function(e) TRUE))
