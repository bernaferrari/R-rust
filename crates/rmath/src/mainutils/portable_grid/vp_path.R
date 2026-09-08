function(...) { p<-as.character(list(...)); if(length(p)==0L || any(!nzchar(p))) stop('invalid viewport path'); structure(paste(p,collapse='::'), class=c('vpPath','path')) }
