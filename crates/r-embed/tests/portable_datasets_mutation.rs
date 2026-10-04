//! Removed-binding behavior matches pinned GNU R r90451. GNU crashes on an
//! integer lazydata environment; the portable runtime deliberately rejects it.
use r_embed::{RSession, RValue, RuntimePathPolicy};

fn no_host() -> RSession {
    RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), "/tmp"))
        .expect("explicit no-host session")
}

fn true_result(session: &mut RSession, source: &str) {
    let output = session.eval_result(source).expect(source);
    assert_eq!(output.value, RValue::Logical(Some(true)), "{source}");
}

#[test]
fn portable_dataset_mutation_removed_lazy_binding_matches_gnu_errors_and_recovers() {
    let mut session = no_host();
    true_result(
        &mut session,
        r#"{
        lazy <- getNamespaceInfo('datasets', 'lazydata')
        saved <- get('mtcars', lazy, inherits=FALSE)
        rm('mtcars', envir=lazy); gc()
        exported <- tryCatch(datasets::mtcars, error=function(e) conditionMessage(e))
        private <- tryCatch(datasets:::mtcars, error=function(e) conditionMessage(e))
        identical(exported, "'mtcars' is not an exported object from 'namespace:datasets'") &&
        identical(private, "object 'mtcars' not found")
    }"#,
    );
    true_result(
        &mut session,
        "assign('mtcars',saved,lazy);identical(datasets::mtcars,saved)&&identical(1L+1L,2L)",
    );
}

#[test]
fn portable_dataset_mutation_non_environment_metadata_is_typed_error_and_recovers() {
    let mut session = no_host();
    true_result(
        &mut session,
        r#"{
        info <- get('.__NAMESPACE__.',getNamespace('datasets'),inherits=FALSE)
        saved <- info$lazydata
        info$lazydata <- 1L; gc()
        failure <- tryCatch(datasets::mtcars,error=function(e) conditionMessage(e))
        is.character(failure) && length(failure)==1L && grepl('environment',failure)
    }"#,
    );
    true_result(
        &mut session,
        "info$lazydata<-saved;identical(dim(datasets::mtcars),c(32L,11L))&&identical(1L+1L,2L)",
    );
}
