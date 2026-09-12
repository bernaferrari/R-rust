invisible(dput(rle(c(1, 1, 2, 2, 2, 3))))
invisible(dput(rle(c("a", "a", "b"))))
invisible(dput(inverse.rle(rle(c(1, 1, 2, 2, 2, 3)))))
cat(tryCatch(rle(matrix(1)), error = function(e) conditionMessage(e)), "\n", sep = "")
