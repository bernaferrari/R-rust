e <- new.env()
print(.Internal(delayedAssign("y", 2, e, e)))
print(exists("y", envir=e))
print(get("y", envir=e))
