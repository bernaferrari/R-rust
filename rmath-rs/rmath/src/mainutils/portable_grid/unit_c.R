function(...) {
  values<-numeric(); units<-character()
  for (x in list(...)) {
    if (!is.unit(x)) stop('all arguments must be units')
    if (!is.null(x$data)) stop('combining data-dependent units is not supported')
    values<-c(values,x$value); units<-c(units,x$units)
  }
  unit(values,units)
}
