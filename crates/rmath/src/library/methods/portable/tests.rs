use crate::{mainutils::paths::RuntimePathPolicy, sexp::RSession};

fn evaluate(code: &str) {
    // Genuine full base and default packages, with no ambient installed R discovery.
    let mut session = RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"));
    let (result, _, _) = session.eval_code_with_output_capture(code);
    let result = result.unwrap_or_else(|error| panic!("{code}: {}", error.message));
    assert_eq!(result.try_logical_elt(0).unwrap(), 1, "{code}");
}

#[test]
fn owned_portable_methods_public_namespace_and_is_match_gnu() {
    evaluate("n <- asNamespace('methods'); identical(getNamespaceName(n), 'methods') && exists('is', n, inherits=FALSE) && is(1L, 'integer') && is(1L, 'numeric') && !is(1L, 'character')");
}

#[test]
fn owned_portable_methods_inherited_specificity_matches_original_browser_case() {
    evaluate("local({setClass('A');setClass('B',contains='A');setClass('C',contains='B');setClass('D',contains=c('A','C'));setGeneric('f',function(x)standardGeneric('f'));setMethod('f','C',function(x)'C');setMethod('f','A',function(x)'A');identical(f(new('D')),'C')})");
}

#[test]
fn owned_portable_methods_any_signatures_match_original_browser_case() {
    evaluate("setGeneric('wild',function(x,y) standardGeneric('wild')); setMethod('wild',c('ANY','ANY'),function(x,y) 'fallback'); setMethod('wild','numeric',function(x,y) 'number'); identical(c(wild(2,NULL),wild(NULL,TRUE)), c('number','fallback'))");
}

#[test]
fn owned_portable_methods_original_internal_parent_calls_preserve_environments() {
    evaluate("e <- new.env(parent=emptyenv()); p <- new.env(parent=baseenv()); identical(.Internal(`parent.env<-`(e,p)), e) && identical(.Internal(parent.env(e)), p)");
}

#[test]
fn owned_portable_methods_class_cache_is_original_environment() {
    evaluate("n <- asNamespace('methods'); is.environment(get('.classTable', n)) && identical(get('.ImplicitGenericsMetaName', n), '.__IG__table')");
}

#[test]
fn owned_methods_identity_scan_does_not_force_unrelated_lazy_bindings() {
    let mut session = RSession::new_without_default_packages();
    let (result, _, _) = session.eval_code_with_output_capture("n <- new.env(parent=baseenv()); count <- 0L; delayedAssign('setMethod', {count <<- count+1L; function(x)x}, assign.env=n); function(x)x");
    let unrelated = result.unwrap().into_owned().unwrap();
    session.with_active(|| unsafe {
        let owner = crate::sexp::owner::OwnerToken::current().unwrap();
        let global = crate::sexp::globals::R_GlobalEnv();
        let n = owner.sexp(crate::sexp::envir::R_findVarInFrame(global, crate::sexp::symbol::Rf_install(c"n".as_ptr()))).unwrap().into_owned().unwrap();
        let instance=owner.as_ptr();
        (*instance).package_namespace_cache.insert("methods".into(), (std::path::PathBuf::from(super::DIRECTORY), n.as_raw()));
        assert!(!crate::eval::closure::is_methods_matchsignature_closure(unrelated.as_raw()));
        let promise=owner.sexp(crate::sexp::envir::R_findVarInFrame(n.as_raw(),crate::sexp::symbol::Rf_install(c"setMethod".as_ptr()))).unwrap().into_owned().unwrap();
        assert_eq!(crate::sexp::accessors::PRVALUE(promise.as_raw()),crate::sexp::globals::R_UnboundValue());
        let count=owner.sexp(crate::sexp::envir::R_findVarInFrame(global,crate::sexp::symbol::Rf_install(c"count".as_ptr()))).unwrap().into_owned().unwrap();
        assert_eq!(count.try_integer_elt(0).unwrap(),0);
        let resolved=owner.sexp(crate::sexp::envir::forcePromise(promise.as_raw())).unwrap().into_owned().unwrap();
        assert!(crate::eval::closure::is_methods_matchsignature_closure(resolved.as_raw()));
        (*instance).package_namespace_cache.remove("methods");
    });
}
