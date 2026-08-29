# as.character(<double>) must render 15 significant digits: stock coerce.c
# pins R_print.digits to DBL_DIG around StringFromReal, independent of
# options("digits"). Also stock condition-object print rendering:
# print.condition (`<class in <call>: msg>` / `<class: msg>`), try-error
# print.default rendering (message string + class/condition attributes),
# warning() returning the message string invisibly with deferred
# "Warning message:" rendering, and conditionMessage on conditions.
print(as.character(1/3))
print(as.character(0.1 + 0.2))
print(as.character(123456789.123456789))
print(as.character(1e-10))
print(as.character(1/7))
print(as.character(0.5))
print(as.character(1e10))
print(as.character(1e20))
print(as.character(1e-5))
options(digits = 4)
print(as.character(1/3))
print(1/3)
print(try(stop("x"), silent = TRUE))
print(simpleError("boom"))
print(structure(list(message = "oops", call = NULL), class = c("myerr", "error", "condition")))
print(structure(list(message = "m", call = quote(f(1))), class = c("simpleError", "error", "condition")))
print(tryCatch(warning("bad thing"), warning = function(c) c))
cat(conditionMessage(simpleError("msg")), "\n")
# warning() returns the message string invisibly (stock do_warning returns
# CAR(args)); deferred warnings render via PrintWarnings with call
# attribution when an enclosing closure frame exists.
f <- function() warning("w2")
v <- warning("w")
stopifnot(identical(v, "w"))
f()
