value <- withRestarts({
  r <- findRestart("foo")
  print(!is.null(r))
  print(inherits(r, "restart"))
  names <- unlist(lapply(computeRestarts(), function(x) x$name))
  print("foo" %in% names)
  11
}, foo = function() 22)
print(value)
print(is.null(findRestart("foo")))
