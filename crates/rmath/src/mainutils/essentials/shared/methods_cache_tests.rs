use super::*;
use crate::sexp::{object::Sexp, session::RSession};

unsafe fn binding(environment: Sexp<'static>, name: &CStr) -> Sexp<'static> {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }.unwrap();
    let symbol = owner.sexp(unsafe { Rf_install(name.as_ptr()) }).unwrap();
    let value = unsafe { crate::sexp::envir::find_var_in_frame_result(environment, symbol) }
        .unwrap()
        .expect("installed methods binding exists");
    let value = if value.typeof_() == SEXPTYPE::PROMSXP {
        unsafe { crate::sexp::envir::force_promise_result(value) }
            .unwrap()
            .unwrap()
    } else {
        value
    };
    value.into_owned().unwrap()
}

#[test]
fn installed_methods_startup_preserves_original_class_and_initializer_caches() {
    let mut session = RSession::new_without_default_packages();
    session.set_library_paths(
        crate::mainutils::paths::RuntimePathPolicy::from_env()
            .library_paths()
            .to_vec(),
    );
    let namespace = session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        owner
            .sexp(load_package_namespace_by_name("methods").unwrap())
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let (result, output, error_output) = session.eval_script_with_output_capture(
        "n <- asNamespace('methods'); c(
            identical(methods:::.getClassesFromCache('envRefClass'), get('.__C__envRefClass', n)),
            identical(methods:::.getClassesFromCache('signature'), get('.__C__signature', n)),
            !exists('initMatrix', n, inherits=FALSE),
            !exists('initArray', n, inherits=FALSE))",
    );
    let result = result.unwrap_or_else(|error| {
        panic!("original methods caches: {error:?}; output={output:?}; errors={error_output:?}")
    });
    assert_eq!(result.len(), 4);
    for index in 0..4 {
        assert_eq!(result.try_logical_elt(index).unwrap(), TRUE);
    }
    session.with_active(|| unsafe {
        let table = binding(namespace.clone(), c".__T__initialize:methods");
        let matrix = binding(table.clone(), c"matrix");
        let array = binding(table, c"array");
        let environment = matrix.try_cloenv().unwrap().into_owned().unwrap();
        assert_eq!(array.try_cloenv().unwrap(), environment);
        assert_ne!(environment, namespace);
        assert_eq!(environment.try_enclos().unwrap(), namespace);
        let matrix_helper = binding(environment.clone(), c"initMatrix");
        let array_helper = binding(environment.clone(), c"initArray");
        assert_eq!(matrix_helper.typeof_(), SEXPTYPE::CLOSXP);
        assert_eq!(array_helper.typeof_(), SEXPTYPE::CLOSXP);
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(matrix.try_cloenv().unwrap(), environment);
        assert_eq!(array.try_cloenv().unwrap(), environment);
        assert_eq!(binding(environment.clone(), c"initMatrix"), matrix_helper);
        assert_eq!(binding(environment, c"initArray"), array_helper);
    });
}
