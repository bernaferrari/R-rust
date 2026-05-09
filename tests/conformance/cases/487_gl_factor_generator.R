z <- gl(2, 3, labels = c("a", "b"))
cat(paste(as.character(z), collapse = "|"), "\n", sep = "")
cat(paste(levels(z), collapse = "|"), "\n", sep = "")
cat(paste(class(z), collapse = "|"), "\n", sep = "")

w <- gl(2, 2, length = 5, ordered = TRUE)
cat(paste(as.character(w), collapse = "|"), "\n", sep = "")
cat(paste(levels(w), collapse = "|"), "\n", sep = "")
cat(paste(class(w), collapse = "|"), "\n", sep = "")
cat(paste(c(is.ordered(w), is.factor(w)), collapse = "|"), "\n", sep = "")

e <- gl(0, 2)
cat(length(e), "\n", sep = "")
cat(paste(levels(e), collapse = "|"), "\n", sep = "")

cat(tryCatch({
  gl(-1, 2)
  "no error"
}, error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch({
  gl(2, -1)
  "no error"
}, error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch({
  gl(2, 2, length = NA)
  "no error"
}, error = function(e) conditionMessage(e)), "\n", sep = "")
cat(tryCatch({
  gl(2, 2, ordered = NA)
  "no error"
}, error = function(e) conditionMessage(e)), "\n", sep = "")
