# sprintf / c / as.character parity with language, symbol, list,
# expression and pairlist arguments (bind.c AnswerType semantics and
# sprintf.c LANGSXP/SYMSXP rejection).
cat(sprintf("%s\n", c("a", quote(f(1)))))
cat(sprintf("%s|", list(quote(f(1)))))
cat(sprintf("%s\n", list(1L, 2.5, NULL, sin)))
cat(sprintf("%s\n", expression(1 + 1)))
cat(sprintf("%s\n", pairlist(1, 2)))
cat(sprintf("%s %s\n", c("x", "y"), c(1, 2)))
cat(sprintf("%d\n", c(1L, 2L)))
msg <- function(e) print(conditionMessage(attr(e, "condition")))
msg(try(sprintf("%s", quote(f(1))), silent = TRUE))
msg(try(sprintf("%d", quote(f(1))), silent = TRUE))
msg(try(sprintf("%e", quote(f(1))), silent = TRUE))
msg(try(sprintf("%s", quote(x)), silent = TRUE))
msg(try(sprintf("%s", 1, quote(f(1))), silent = TRUE))
msg(try(sprintf("%d", list(1, quote(f(1)))), silent = TRUE))
msg(try(sprintf("%d", expression(1 + 1)), silent = TRUE))
