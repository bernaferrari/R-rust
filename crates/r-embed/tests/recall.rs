use r_embed::RSession;

#[test]
fn recall_preserves_closure_identity_missing_arguments_and_visibility() {
    let mut session = RSession::new().unwrap();
    for code in [
        "identical(typeof(Recall),'closure') && identical(names(formals(Recall)),'...')",
        "local({f<-function(n,unused=stop('unused'))if(n>0)Recall(n-1)else 42L;identical(f(3),42L)})",
        "local({f<-function(n){f<-function(...)99L;if(n>0)Recall(n-1)else 42L};identical(f(2),42L)})",
        "local({f<-function(n,x)if(n>0)Recall(n-1)else missing(x);isTRUE(f(2))})",
        "local({f<-function(n)if(n>0)Recall(n-1)else invisible(42L);identical(withVisible(f(2)),list(value=42L,visible=FALSE))})",
    ] {
        assert_eq!(session.eval(code).unwrap().trim(), "[1] TRUE", "{code}");
    }
}

#[test]
fn recall_at_top_level_errors_and_session_recovers() {
    let mut session = RSession::new().unwrap();
    assert!(
        session
            .eval("Recall()")
            .unwrap_err()
            .to_string()
            .contains("outside a closure")
    );
    assert_eq!(session.eval("1+1").unwrap().trim(), "[1] 2");
}
