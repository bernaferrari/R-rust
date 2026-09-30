function(x, units, data=NULL) {
  if (length(x)==0L || length(units)==0L) stop("'x' and 'units' must have length > 0")
  if (!is.numeric(x) || !is.character(units)) stop('invalid grid unit')
  structure(list(value=as.numeric(x), units=rep_len(units,length(x)), data=data), class='unit')
}
