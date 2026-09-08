options(digits=17)
n<-36; i<-seq_len(n)
for(d in 1:4) for(surface in c('direct','interpolate')) for(family in c('gaussian','symmetric')) {
 x<-sapply(seq_len(d),function(j)sin(i*(j*0.391+0.123))+i*(0.013*j))
 y<-sin(x[,1]*2)+rowSums(x^2)*0.21+cos(i)*0.04;y[9]<-y[9]+1
 f<-loess(y~x,span=.9,degree=2,family=family,control=loess.control(surface=surface,statistics='exact'))
 cat(d,surface,family,'\n');cat(paste(f$fitted,collapse=','),'\n');cat(f$trace.hat,f$one.delta,f$two.delta,f$s,'\n');cat(paste(f$robust,collapse=','),'\n')
}
