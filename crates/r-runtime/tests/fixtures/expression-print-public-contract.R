x <- quote(f(0.3))
stopifnot(identical(capture.output(print(x)), "f(0.3)"))
stopifnot(identical(capture.output(print(expression(f(0.3)))), "expression(f(0.3))"))
stopifnot(identical(deparse(x), "f(0.3)"))
stopifnot(identical(deparse(x, control = "all"), "quote(f(0.29999999999999999))"))
stopifnot(identical(x, eval(parse(text = deparse(x, control = "all")))))
TRUE
