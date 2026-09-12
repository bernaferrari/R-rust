invisible(dput(outer(1:2, 1:2, "+")))
invisible(dput(outer(1:2, 1:3, "*")))
invisible(dput(outer(c(a = 1, b = 2), 1:2, "+")))
cat(tryCatch(outer(1:2, 1:2, "notafun"), error = function(e) conditionMessage(e)), "\n", sep = "")
