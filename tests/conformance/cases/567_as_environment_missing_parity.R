## as.environment(list) and free-variable missing semantics

## as.environment on a named list: an environment with the list's
## bindings, parent emptyenv() (whisker's partials path).
e <- as.environment(list(p = "PP", n = 2))
cat(ls(e), e$p, e$n, "\n", sep = "|")
cat(identical(parent.env(e), emptyenv()), "\n", sep = "")
cat(e[["p"]], e[["n"]], "\n", sep = "|")
## Unnamed list is an error with the exact upstream message.
msg <- tryCatch(as.environment(list(1, 2)), error = function(c) conditionMessage(c))
cat(msg, "\n", sep = "")

## R_isMissing: a symbol NOT bound in the queried frame is simply not
## missing — the check exists for the current call's formals. Free
## variables from ENCLOSING frames must evaluate normally through
## argument lists (whisker's renderPartial closes over partial()'s key).
ff <- function(key) function() key
cat(ff("p")(), "\n", sep = "")
gg <- function(env) function(k) env[[k]]
ee <- as.environment(list(p = "PP"))
cat(gg(ee)("p"), "\n", sep = "")

## A genuinely missing formal still reports missing through the promise.
hh <- function(a) function() a
mm <- tryCatch(hh()(), error = function(c) conditionMessage(c))
cat(grepl("missing", mm), "\n", sep = "")
