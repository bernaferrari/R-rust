# Native .C oracle for GNU R r90451, source bac583951b728e97b9786804d3b4081f0fe18df5.
# Run with the pinned Rscript and one output TSV argument.
args <- commandArgs(TRUE)
stopifnot(length(args) == 1L)
options(warn=2)
entry <- getDLLRegisteredRoutines('stats')$.C$HoltWinters
stopifnot(entry$numParameters == 17L)
cases <- list(
 additive=list(x=c(20,19,21,23,20,24,26), alpha=.3,beta=.1,gamma=.2,start=2L,kind=1L,period=3L,dt=1L,ds=1L),
 multiplicative=list(x=c(20,19,21,23,20,24,26),alpha=.3,beta=.1,gamma=.2,start=2L,kind=2L,period=3L,dt=1L,ds=1L),
 no_trend=list(x=c(20,19,21,23),alpha=.3,beta=.1,gamma=.2,start=1L,kind=1L,period=3L,dt=0L,ds=1L),
 no_season=list(x=c(20,19,21,23),alpha=.3,beta=.1,gamma=.2,start=1L,kind=2L,period=0L,dt=1L,ds=0L),
 other_flags=list(x=c(20,19,21,23),alpha=.3,beta=.1,gamma=.2,start=1L,kind=0L,period=0L,dt=2L,ds=2L),
 zero_period=list(x=c(20,19,21,23),alpha=.3,beta=.1,gamma=.2,start=1L,kind=1L,period=0L,dt=1L,ds=1L),
 no_iterations=list(x=c(20,19,21),alpha=.3,beta=.1,gamma=.2,start=6L,kind=1L,period=3L,dt=1L,ds=1L),
 empty=list(x=double(),alpha=.3,beta=.1,gamma=.2,start=1L,kind=2L,period=0L,dt=0L,ds=0L),
 boundary_coefficients=list(x=c(20,19,21,23),alpha=1,beta=0,gamma=0,start=1L,kind=1L,period=2L,dt=1L,ds=1L)
)
rows <- list()
encode <- function(x) paste(format(x,digits=17,scientific=TRUE,trim=TRUE),collapse=',')
for(nm in names(cases)) {
 p<-cases[[nm]]; n<-length(p$x); steps<-max(n-p$start+1L,0L)
 trend_len<-if(p$dt==1L) steps+1L else steps
 seed <- rep(c(.5,1.5,-.25),length.out=p$period)
 level<-rep(-111,steps+3L); trend<-rep(5.25,trend_len+2L); season<-rep(2.25,p$period+steps+2L)
 input<-list(x=p$x,xl=as.integer(n),alpha=p$alpha,beta=p$beta,gamma=p$gamma,start_time=p$start,seasonal=p$kind,period=p$period,dotrend=p$dt,doseasonal=p$ds,a=17.5,b=-.5,s=seed,SSE=3.75,level=level,trend=trend,season=season)
 output<-do.call(.C,c(list(entry),input,list(NAOK=TRUE)))
 stopifnot(length(output)==17L)
 for(i in seq_along(input)) rows[[length(rows)+1L]]<-data.frame(case=nm,index=i-1L,type=if(is.integer(input[[i]]))'integer'else'real',input=encode(input[[i]]),output=encode(output[[i]]))
}
write.table(do.call(rbind,rows),file=args[[1L]],sep='\t',quote=FALSE,row.names=FALSE,na='')
cat('PASS:',length(cases),'independent HoltWinters cases;',length(rows),'argument records\n')
