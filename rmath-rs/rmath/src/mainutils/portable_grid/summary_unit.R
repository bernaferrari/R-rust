function(..., na.rm=FALSE) {
  z <- list(...)
  if (!length(z) || !all(vapply(z,is.unit,logical(1)))) stop('unit summary requires unit arguments')
  u <- z[[1]]$units
  if (any(vapply(z,function(x) !identical(x$units,u),logical(1)))) stop('unit summary requires matching units')
  vals <- unlist(lapply(z,function(x)x$value), use.names=FALSE)
  unit(switch(.Generic, 'sum'=sum(vals,na.rm=na.rm), 'min'=min(vals,na.rm=na.rm), 'max'=max(vals,na.rm=na.rm), stop(paste('unit summary',.Generic,'is not supported'))), u)
}
