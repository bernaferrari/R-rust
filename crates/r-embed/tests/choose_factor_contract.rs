use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
expected <- "non-numeric argument to mathematical function"
for(f in list(choose,lchoose))for(x in list(factor("a"),ordered(c("a","b")),factor(character(0)),structure(1L,class=c("foo","factor"),levels="a"))) {
 for(y in list(1,1.2,numeric(0))) {
  events <- character()
  message <- withCallingHandlers(tryCatch(f(x,y),error=conditionMessage),warning=function(w){events <<- c(events,conditionMessage(w));invokeRestart("muffleWarning")})
  stopifnot(identical(message,expected),identical(events,character(0)),identical(tryCatch(f(y,x),error=conditionMessage),expected))
 }
}
stopifnot(identical(choose(structure(1L,levels="a"),1),structure(1,levels="a")))
x <- structure(c(5L,6L),names=c("a","b"),class="foo")
stopifnot(identical(choose(x,2),structure(c(10,15),names=c("a","b"),class="foo")))
stopifnot(identical(choose(TRUE,1),1),identical(choose(numeric(0),1),numeric(0)))
stopifnot(identical(tryCatch(choose(1+1i,factor("a")),error=conditionMessage),"unimplemented complex function"))
message <- tryCatch(choose(factor("a"),1),error=function(e){gc();conditionMessage(e)})
stopifnot(identical(message,expected),identical(choose(6,2),15))
cat("choose-factor-contract-ok\n")
"#;
#[test]
fn public_choose_factor_validation() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("choose-factor-contract-ok"), "{out}");
    }
}
