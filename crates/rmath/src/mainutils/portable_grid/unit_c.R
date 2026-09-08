function(...) {
  values<-numeric(); units<-character(); data<-list(); has.data<-FALSE
  for (x in list(...)) {
    if (!is.unit(x)) stop('all arguments must be units')
    values<-c(values,x$value); units<-c(units,x$units)
    if (is.null(x$data)) data<-c(data,rep(list(NULL),length(x$value))) else { has.data<-TRUE; data<-c(data,rep(x$data,length.out=length(x$value))) }
  }
  unit(values,units,if(has.data) data else NULL)
}
