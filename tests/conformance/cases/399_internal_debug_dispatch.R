f <- function(x) x + 1
print(.Internal(isdebugged(f)))
print(.Internal(debug(f, FALSE, FALSE)))
print(.Internal(isdebugged(f)))
print(.Internal(debugonce(f, FALSE, FALSE)))
print(.Internal(isdebugged(f)))
print(.Internal(undebug(f)))
print(.Internal(isdebugged(f)))
