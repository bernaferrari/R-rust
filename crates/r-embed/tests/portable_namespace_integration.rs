use r_embed::{RSession, RuntimePathPolicy};

#[test]
fn portable_methods_exports_are_available_through_public_constructor() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    assert_eq!(
        session
            .eval("n <- asNamespace('methods'); identical(getNamespaceName(n), 'methods') && is(1L, 'integer') && is(1L, 'numeric') && !is(1L, 'character')")
            .unwrap(),
        "[1] TRUE\n"
    );
}

#[test]
fn portable_utils_exports_are_available_through_public_constructor() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    assert_eq!(
        session
            .eval("n <- asNamespace('utils'); identical(getNamespaceName(n), 'utils') && identical(environment(utils:::defaultUserAgent), n) && identical(environment(utils:::.osVersion), n) && is.list(getOption('str')) && is.character(getOption('HTTPUserAgent')) && identical(utils::head(1:8, 3L), 1:3) && identical(head(1:8, 3L), 1:3)")
            .unwrap(),
        "[1] TRUE\n"
    );
    assert!(
        session
            .eval("system('echo must-not-execute')")
            .unwrap_err()
            .to_string()
            .contains("disabled by the session capability policy")
    );
    assert_eq!(session.eval("head(1:3, 1L)").unwrap(), "[1] 1\n");
    assert_eq!(session.eval("w <- tryCatch(warning('metadata', call.=FALSE), warning=identity); identical(conditionMessage(w), 'metadata') && is.null(conditionCall(w))").unwrap(), "[1] TRUE\n");
    assert!(
        session
            .eval("stop('original error',call.=FALSE)")
            .unwrap_err()
            .to_string()
            .contains("original error")
    );
    assert_eq!(session.eval("head(1:3,1L)").unwrap(), "[1] 1\n");
}

#[test]
fn portable_methods_inherited_and_any_dispatch_use_original_namespace() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp")).unwrap();
    assert_eq!(session.eval("setGeneric('wild',function(x,y) standardGeneric('wild')); setMethod('wild',c('ANY','ANY'),function(x,y) 'fallback'); setMethod('wild','numeric',function(x,y) 'number'); cat(wild(2,NULL),wild(NULL,TRUE))").unwrap(), "[1] \"wild\"\nnumber fallback");
    assert_eq!(session.eval("local({setClass('SelectA');setClass('SelectB',contains='SelectA');setGeneric('selectprobe',function(x)standardGeneric('selectprobe'));setMethod('selectprobe','SelectA',function(x)42L);m<-selectMethod('selectprobe','SelectB');c(result=identical(m(new('SelectB')),42L),defined=identical(as.character(m@defined),'SelectA'))})").unwrap(), " result defined \n   TRUE    TRUE \n");
}
