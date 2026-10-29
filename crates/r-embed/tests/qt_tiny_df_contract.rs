use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
for(lower in c(TRUE,FALSE))for(logp in c(TRUE,FALSE)) {
 p <- c(.1,.5,.9)
 if(logp)p <- log(p)
 events <- character()
 z <- withCallingHandlers(qt(p,1e-300,lower.tail=lower,log.p=logp),warning=function(w){events <<- c(events,conditionMessage(w));gc();invokeRestart("muffleWarning")})
 target <- if(lower)c(-Inf,NaN,Inf)else c(Inf,NaN,-Inf)
 stopifnot(identical(z,target),identical(events,"NaNs produced"))
}
stopifnot(identical(qt(c(.025,.975),.001),c(-Inf,Inf)))
z <- qt(c(.25,.75),.001)
stopifnot(all(abs(z/c(-1.6949002133388867e299,1.6949002133417418e299)-1)<1e-10))
z <- qt(c(.1,.25,.75,.9),.1)
stopifnot(all(abs(z/c(-1.6044257056665733e6,-168.23607319770966,168.23607319770966,1.6044257056665733e6)-1)<1e-10))
stopifnot(abs(qt(.5,1e-10)-1.3877787808781017e-11)<1e-20)
x <- structure(c(.1,NA_real_,NaN,.9),names=c("left","missing","nan","right"))
z <- qt(x,1e-300);gc()
stopifnot(identical(names(z),names(x)),identical(unname(z[c(1,4)]),c(-Inf,Inf)),is.na(z[2]),!is.nan(z[2]),is.nan(z[3]))
stopifnot(identical(qt(c(-Inf,0),1e-300,log.p=TRUE),c(-Inf,Inf)))
cat("qt-tiny-df-contract-ok\n")
"#;
#[test]
fn public_tiny_df_quantiles() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("qt-tiny-df-contract-ok"), "{out}");
    }
}
