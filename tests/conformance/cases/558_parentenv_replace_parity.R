# `parent.env(env) <- value` replacement form re-parents environments.
e <- new.env()
parent.env(e) <- globalenv()
cat("reparent:", identical(parent.env(e), globalenv()), "\n")
