function(..., na.rm=FALSE) {
  z <- list(...)
  if (!length(z) || !all(vapply(z,is.unit,logical(1)))) stop('unit summary requires unit arguments')
  u <- z[[1]]$units
  if (any(vapply(z,function(x) !is.unit(x),logical(1)))) stop('unit summary requires unit arguments')
  vals <- unlist(lapply(z,function(x)x$value), use.names=FALSE)
  mixed <- length(z)>1L && any(vapply(z,function(x)!identical(x$units,u),logical(1)))
  dependent <- any(vapply(z,function(x)!is.null(x$data),logical(1)))
  if (mixed || dependent) return(structure(list(value=1,units=.Generic,data=unlist(lapply(z,function(x) lapply(seq_along(x$value),function(i) unit(x$value[i],x$units[i],if(is.null(x$data)) NULL else x$data[[i]]))),recursive=FALSE)),class='unit'))
  unit(switch(.Generic, 'sum'=sum(vals,na.rm=na.rm), 'min'=min(vals,na.rm=na.rm), 'max'=max(vals,na.rm=na.rm), stop(paste('unit summary',.Generic,'is not supported'))), u)
}
