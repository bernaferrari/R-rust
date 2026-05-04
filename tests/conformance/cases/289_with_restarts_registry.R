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
print(withRestarts({
  invokeRestart("foo", 21)
  0
}, foo = function(x) x + 1))
print(withRestarts({
  invokeRestart(findRestart("bar"), 10)
  0
}, bar = function(x) x * 2))
