use r_embed::{RSession, RuntimePathPolicy};

fn portable() -> RSession {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    session.enable_browser_files();
    session
}

#[test]
fn loess_models_remain_independent_after_refits_and_serialization() {
    for portable_policy in [false, true] {
        let mut session = if portable_policy {
            portable()
        } else {
            RSession::new().unwrap()
        };
        assert_eq!(
            session
                .eval(include_str!("fixtures/loess-isolation.R"))
                .unwrap(),
            "[1] TRUE\n"
        );
    }
}

#[test]
fn loess_surface_survives_original_session_close_and_fresh_session_import() {
    let mut original = portable();
    original.eval("x<-seq(0,1,length.out=30);y<-sin(5*x);f<-loess(y~x);q<-c(.15,.45,.75);expected<-predict(f,q);saveRDS(list(model=f,q=q,expected=expected),'model.rds',compress=FALSE)").unwrap();
    let bytes = original.export_file("model.rds").unwrap();
    original.close();
    drop(original);
    let mut restored = portable();
    restored.import_file("model.rds", &bytes).unwrap();
    assert_eq!(
        restored
            .eval(
                "saved<-readRDS('model.rds');identical(saved$expected,predict(saved$model,saved$q))"
            )
            .unwrap(),
        "[1] TRUE\n"
    );
}

#[test]
fn loess_imports_independently_serialized_gnu_surface_without_any_prior_fit() {
    let mut session = portable();
    session
        .import_file("gnu.rds", include_bytes!("fixtures/gnu-loess-surface.rds"))
        .unwrap();
    assert_eq!(session.eval("saved<-readRDS('gnu.rds');max(abs(saved$expected-predict(saved$model,saved$q)))<1e-9").unwrap(),"[1] TRUE\n");
}
