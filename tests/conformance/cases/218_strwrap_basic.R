print(paste(strwrap("alpha beta gamma", width = 10), collapse = "|"))
print(paste(strwrap(c("alpha beta gamma", "delta epsilon"), width = 10), collapse = "|"))
print(paste(strwrap(NA_character_, width = 10), collapse = "|"))
print(paste(strwrap("supercalifragilistic", width = 5), collapse = "|"))
