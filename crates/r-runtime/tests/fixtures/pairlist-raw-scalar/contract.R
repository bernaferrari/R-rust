for (mode in c("double", "complex")) {
  helper <- if (mode == "double") "asReal" else "asComplex"
  expected_error <- paste0("unimplemented type 'raw' in '", helper, "'\n")
  for (input in list(pairlist(as.raw(7)), list(as.raw(7)))) {
    message <- tryCatch(as.vector(input, mode), error = function(e) conditionMessage(e))
    stopifnot(identical(message, expected_error))
  }
  empty <- if (mode == "double") NA_real_ else NA_complex_
  positive <- if (mode == "double") 7 else 7 + 0i
  stopifnot(identical(as.vector(pairlist(raw()), mode), empty))
  stopifnot(identical(as.vector(list(raw()), mode), empty))
  stopifnot(identical(as.vector(as.raw(7), mode), positive))
  cat(mode, "raw-child-error-and-neighbors=PASS\n")
}
stopifnot(identical(as.vector(pairlist(as.raw(7)), "logical"), TRUE))
stopifnot(identical(as.vector(pairlist(as.raw(7)), "integer"), 7L))
stopifnot(identical(as.vector(pairlist(as.raw(7)), "raw"), as.raw(7)))
TRUE
