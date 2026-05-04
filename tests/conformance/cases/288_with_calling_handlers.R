warnings <- character()
messages <- character()

print(suppressWarnings(withCallingHandlers({
  warning("careful")
  7
}, warning = function(w) {
  warnings <<- c(warnings, conditionMessage(w))
})))
print(warnings)

print(suppressMessages(withCallingHandlers({
  message("hello")
  9
}, message = function(m) {
  messages <<- c(messages, conditionMessage(m))
})))
print(grepl("hello", messages))
print(nchar(messages))
