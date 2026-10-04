//! The original GNU getNamespaceExports contract through real public base code.
use r_embed::{RSession, RValue, RuntimePathPolicy};

#[test]
fn namespace_exports_matches_gnu_base_ordinary_and_invalid_namespaces() {
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        .expect("real base without installed packages");
    let check = |session: &mut RSession, code: &str| {
        assert_eq!(
            session.eval_result(code).expect(code).value,
            RValue::Logical(Some(true)),
            "{code}"
        );
    };
    check(
        &mut session,
        "exports<-getNamespaceExports('base');identical(exports,names(.BaseNamespaceEnv))&&'+'%in%exports",
    );
    check(
        &mut session,
        r#"{
        ns<-new.env(parent=baseenv());info<-new.env(parent=baseenv());exported<-new.env(parent=baseenv())
        exported$beta<-TRUE;exported$alpha<-TRUE;info$exports<-exported
        assign('.__NAMESPACE__.',info,ns)
        identical(sort(getNamespaceExports(ns)),c('alpha','beta'))
    }"#,
    );
    check(
        &mut session,
        "info$exports<-new.env(parent=baseenv());identical(getNamespaceExports(ns),character())",
    );
    assert!(session.eval("getNamespaceExports('no_such_pkg')").is_err());
    assert!(session.eval("getNamespaceExports(globalenv())").is_err());
    check(
        &mut session,
        "identical(getNamespaceExports(ns),character())&&'+'%in%getNamespaceExports('base')",
    );
}
