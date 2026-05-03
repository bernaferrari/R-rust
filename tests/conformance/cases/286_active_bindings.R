e <- new.env()
seen <- 0
value <- 10
makeActiveBinding("x", function(v) {
  if (missing(v)) {
    seen <<- seen + 1
    value
  } else {
    value <<- v
    invisible(NULL)
  }
}, e)

print(bindingIsActive("x", e))
print(exists("x", e, inherits = FALSE))
print(seen)
print(e$x)
assign("x", 42, envir = e)
print(e$x)
print(seen)
print(tryCatch({
  lockBinding("x", e)
  e$x <- 3
  "ok"
}, error = function(err) "locked"))
