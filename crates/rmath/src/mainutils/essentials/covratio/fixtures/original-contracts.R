# Pinned GNU R revision bac583951b728e97b9786804d3b4081f0fe18df5.
# Run its Rscript --vanilla original-contracts.R; stdout reproduces original-contracts.txt.
# These independent outputs supply the numeric inputs and expected results in covratio_tests.rs.
d <- data.frame(y=c(1,3,2,5,4,7,6,9),x=1:8,row.names=paste0("row",1:8))
m <- lm(y~x,d)
i <- lm.influence(m,do.coef=FALSE)
dump <- function(label,x) { cat(label,"=",paste(sprintf("%.17g",x),collapse=",")," names=",paste(names(x),collapse=","),"\n",sep="") }
dump("residuals",residuals(m)); dump("hat",i$hat); dump("sigma",i$sigma);dump("rank",m$rank);dump("model_sigma",sigma(m));dump("default",covratio(m))
dump("optional",covratio(m,infl=list(hat=rep(0.2,8),sigma=rep(2,8)),res=setNames(1:8,paste0("res",1:8))))
dump("recycled",covratio(m,infl=list(hat=setNames(c(.1,.2),c("hat1","hat2")),sigma=c(2,3)),res=1:8))
cat("rankzero_error=",tryCatch(covratio(lm(y~0,d)),error=conditionMessage),"\n",sep="")
dump("dfzero",covratio(lm(c(1,2)~1)))
