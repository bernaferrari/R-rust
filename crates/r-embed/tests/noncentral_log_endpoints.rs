use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
funs <- list(function(p,n,lower,logp)qbeta(p,1,2,ncp=n,lower.tail=lower,log.p=logp),function(p,n,lower,logp)qchisq(p,1,ncp=n,lower.tail=lower,log.p=logp),function(p,n,lower,logp)qt(p,1,ncp=n,lower.tail=lower,log.p=logp),function(p,n,lower,logp)qf(p,1,2,ncp=n,lower.tail=lower,log.p=logp))
low <- c(0,0,-Inf,0);high <- c(1,Inf,Inf,Inf)
for(i in 1:4)for(n in c(0,1,10))for(lower in c(TRUE,FALSE)) {
 target <- if(lower)c(low[i],high[i])else c(high[i],low[i])
 stopifnot(identical(funs[[i]](c(-Inf,0),n,lower,TRUE),target),identical(funs[[i]](c(0,1),n,lower,FALSE),target))
}
expected <- list(c(.080936850346656874,.392340733604363945,.763469405254951217),c(.042701245444327726,1.103643311336762745,5.218794098621963862),c(-.39606636500765546,1.32021309938873799,8.57049371094650780),c(.054385032947771104,1.561116699069021063,17.505096105835278308))
for(i in 1:4) {
 z <- funs[[i]](c(.1,.5,.9),1,TRUE,FALSE)
 stopifnot(all(abs(z-expected[[i]])<1e-10))
 z <- funs[[i]](log(c(.1,.5,.9)),1,TRUE,TRUE)
 stopifnot(all(abs(z-expected[[i]])<1e-10))
 x <- structure(c(-Inf,0,NA_real_,NaN),names=c("zero","one","missing","nan"))
 z <- funs[[i]](x,1,TRUE,TRUE);gc()
 stopifnot(identical(names(z),names(x)),is.na(z[3]),!is.nan(z[3]),is.nan(z[4]))
 events <- character()
 z <- withCallingHandlers(funs[[i]](c(-Inf,1),1,TRUE,TRUE),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
 stopifnot(identical(z[1],low[i]),is.nan(z[2]),identical(events,"NaNs produced"))
}
cat("noncentral-log-endpoints-ok\n")
"#;
#[test]
fn public_noncentral_log_endpoints() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("noncentral-log-endpoints-ok"), "{out}");
    }
}
