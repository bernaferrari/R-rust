options(digits=17); n<-36;i<-seq_len(n);x1<-sin(i*.514)+i*.013;x2<-sin(i*.905)+i*.026;y<-sin(x1*2)+(x1^2+x2^2)*.21+cos(i)*.04;y[9]<-y[9]+1;w<-1+(i%%5)/3
for(degree in 0:2) for(surface in c('direct','interpolate')){
 f<-loess(y~x1+x2,weights=w,degree=degree,span=1.3,normalize=FALSE,parametric=c(FALSE,TRUE),drop.square=c(degree==2,FALSE),family='symmetric',control=loess.control(surface=surface,statistics='approximate'))
 cat(degree,surface,'\n');cat(paste(f$fitted,collapse=','),'\n');cat(f$trace.hat,f$one.delta,f$two.delta,f$s,'\n');cat(paste(predict(f,se=TRUE)$se.fit,collapse=','),'\n')
}
