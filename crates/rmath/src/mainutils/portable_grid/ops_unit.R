function(e1, e2=NULL) {
  op <- .Generic
  if (op %in% c('+','-')) {
    if (is.null(e2) || !is.unit(e1) || !is.unit(e2)) stop('unit addition requires unit operands')
    if (identical(e1$units,e2$units) && is.null(e1$data) && is.null(e2$data)) return(unit(if (op == '+') e1$value + e2$value else e1$value-e2$value,e1$units))
    if (op == '-') e2$value <- -e2$value
    return(structure(list(value=1,units='sum',data=list(e1,e2)),class='unit'))
  }
  if (op %in% c('*','/')) {
    if (is.null(e2)) stop('invalid unit arithmetic')
    if (is.unit(e1) && is.numeric(e2) && length(e2)==1L) return(unit(if(op=='*') e1$value*e2 else e1$value/e2,e1$units,e1$data))
    if (op=='*' && is.numeric(e1) && length(e1)==1L && is.unit(e2)) return(unit(e1*e2$value,e2$units,e2$data))
    stop('unit multiplication and division require one scalar numeric operand')
  }
  if (op %in% c('==','!=','<','<=','>','>=')) {
    if (!is.unit(e1) || !is.unit(e2) || !identical(e1$units,e2$units)) stop('unit comparison requires matching units')
    return(switch(op, '=='=e1$value==e2$value, '!='=e1$value!=e2$value, '<'=e1$value<e2$value, '<='=e1$value<=e2$value, '>'=e1$value>e2$value, '>='=e1$value>=e2$value))
  }
  stop(paste('unit operator',op,'is not supported'))
}
