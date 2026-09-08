x<-seq(0,1,length.out=30); y<-sin(5*x)+x^2; y[c(4,17)]<-NA
f<-loess(y~x,na.action=na.exclude); g<-loess(y~x,na.action=na.omit)
cat(class(f$na.action),paste(as.integer(f$na.action),collapse=','),length(predict(f)),paste(which(is.na(predict(f))),collapse=','),length(predict(g)),all.equal(predict(f)[-c(4,17)],predict(g)),'\n')
p<-predict(f,newdata=data.frame(x=c(.2,NA,.8))); q<-predict(f,se=TRUE)
h<-unserialize(serialize(f,NULL))
cat(length(p),paste(which(is.na(p)),collapse=','),length(q$fit),length(predict(h)),paste(which(is.na(predict(h))),collapse=','),'\n')
p<-predict(g,c(NA,NaN,Inf,-1),se=TRUE)
cat(all(is.na(p$fit)),any(is.nan(p$fit)),all(is.na(p$se.fit)),any(is.nan(p$se.fit)),'\n')
