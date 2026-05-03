outer <- function(x) {
  inner <- function(y, ...) {
    list(
      nargs = nargs(),
      nframe = sys.nframe(),
      call = deparse(sys.call()),
      frame_is_env = is.environment(sys.frame(sys.nframe())),
      fun_is_function = is.function(sys.function()),
      parent_has_x = exists("x", envir = parent.frame(), inherits = FALSE)
    )
  }
  inner(x + 1, z = 3)
}

out <- outer(4)
print(out$nargs)
print(out$nframe >= 2)
print(out$call)
print(out$frame_is_env)
print(out$fun_is_function)
print(out$parent_has_x)
