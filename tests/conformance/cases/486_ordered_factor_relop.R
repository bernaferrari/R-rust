x <- ordered(c("low", "high", "medium", NA), levels = c("low", "medium", "high"))
cat(paste(x < "high", collapse = "|"), "\n", sep = "")
cat(paste(x >= ordered("medium", levels = c("low", "medium", "high")), collapse = "|"), "\n", sep = "")
cat(paste(x == "missing", collapse = "|"), "\n", sep = "")

y <- ordered(c("high", "medium", "low", "high"), levels = c("low", "medium", "high"))
cat(paste(x < y, collapse = "|"), "\n", sep = "")
cat(tryCatch({
  x < ordered("a", levels = c("a", "b"))
  "no error"
}, error = function(e) conditionMessage(e)), "\n", sep = "")
