function(string) {
  n <- length(string)
  if (is.language(string)) {
    string <- as.expression(string)
    data <- vector("list", n)
    for (i in 1L:n) data[[i]] <- string[i]
  } else {
    data <- as.list(as.character(string))
  }
  unit(rep_len(1, n), "strheight", data=data)
}
