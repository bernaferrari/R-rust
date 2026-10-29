use r_embed::{RSession, RuntimePathPolicy};
// Pinned GNU R bac583951b728e97b9786804d3b4081f0fe18df5 r90451.
const CONTROLS: &str = r#"
nonnumeric <- "non-numeric argument to mathematical function"
complex.error <- "unimplemented complex function"
for(f in list(choose,lchoose)) {
 for(x in list(NULL,"5",list(5),as.raw(1),expression(5),new.env(),function()5)) {
  for(y in list(1,numeric(0),1+1i)) {
   stopifnot(identical(tryCatch(f(x,y),error=conditionMessage),nonnumeric),identical(tryCatch(f(y,x),error=conditionMessage),if(is.complex(y))complex.error else nonnumeric))
  }
 }
 stopifnot(identical(tryCatch(f(1+1i,1),error=conditionMessage),complex.error),identical(tryCatch(f(numeric(0),complex(0)),error=conditionMessage),nonnumeric))
 stopifnot(identical(f(numeric(0),1),numeric(0)),identical(f(1,numeric(0)),numeric(0)))
 stopifnot(identical(tryCatch(f("bad",1),error=function(e){gc();conditionMessage(e)}),nonnumeric))
}
stopifnot(identical(choose(c(5,6),2),c(10,15)),all(abs(lchoose(c(5,6),2)-log(c(10,15)))<1e-14),identical(choose(TRUE,1),1))
x <- structure(c(5,6),names=c("a","b"),class="foo")
z <- choose(x,2);gc()
stopifnot(identical(z,structure(c(10,15),names=c("a","b"),class="foo")))
stopifnot(identical(choose(c(NA_real_,NaN),1),c(NA_real_,NaN)))
cat("choose-validation-contract-ok\n")
"#;
#[test]
fn public_choose_validation() {
    let portable = RuntimePathPolicy::new(Vec::new(), std::env::temp_dir());
    for mut s in [
        RSession::new().unwrap(),
        RSession::new_with_path_policy(portable).unwrap(),
    ] {
        let out = s.eval(CONTROLS).unwrap();
        assert!(out.contains("choose-validation-contract-ok"), "{out}");
    }
}
