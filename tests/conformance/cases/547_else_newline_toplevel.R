# `else` after a newline only attaches to its `if` inside `(`/`[`/`{`
# (gram.y IfPush); at top level the newline terminates the `if`.

ef <- function(s) tryCatch({deparse(parse(text = s)[[1]])},
  error = function(e)
    paste("ERR else-unexpected:", grepl("unexpected 'else'",
      conditionMessage(e))))
cat("toplevel-sameline:", ef("if (TRUE) 1 else 2\n"), "\n")
cat("toplevel-nl:", ef("if (TRUE)\n1\nelse 2\n"), "\n")
cat("in-braces:", ef("{\nif (TRUE) 1\nelse 2\n}\n"), "\n")
ip <- (if (TRUE) 1
else 2)
cat("in-parens:", ip, "\n")
fnx <- eval(parse(text = "function(a) {\nif (a) 1\nelse 2\n}\n"))
cat("nested-brace-fn:", fnx(FALSE), "\n")
v <- { if (FALSE) 1
else 2 }
cat("eval-braced:", v, "\n")
err <- tryCatch(eval(parse(text = "if (FALSE) 1\nelse 2")), error = function(e)
  grepl("unexpected 'else'", conditionMessage(e)))
cat("eval-toplevel:", err, "\n")
