//! Execute conditional NAMESPACE directives with metadata-only collectors.
use crate::sexp::{object::Sexp, owner::OwnerToken};

pub(super) fn selected_source(content: &str) -> Result<String, String> {
    let owner = unsafe { OwnerToken::current() }.map_err(|error| error.to_string())?;
    let _pin = owner.pin().map_err(|error| error.to_string())?;
    let factory = owner.node_factory();
    let environment = unsafe {
        factory.wrap(crate::sexp::memory_ext::NewEnvironment(
            crate::sexp::globals::R_NilValue(),
            crate::sexp::envir::R_BaseNamespace(),
            crate::sexp::globals::R_NilValue(),
        ))
    }
    .map_err(|error| error.to_string())?;
    let source = format!(
        r#"
        .calls <- character()
        export <- exportMethods <- exportClasses <- exportPattern <-
            import <- importFrom <- importClassesFrom <- importMethodsFrom <-
            S3method <- useDynLib <- function(...) {{
                .calls <<- c(.calls, paste(deparse(sys.call()), collapse=""))
                invisible(NULL)
            }}
        {content}
        .calls
    "#
    );
    let source = std::ffi::CString::new(source).map_err(|_| "NAMESPACE contains NUL")?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        owner
            .sexp(crate::mainutils::gram_main::R_ParseEvalString(
                source.as_ptr(),
                environment.as_raw(),
            ))
            .and_then(Sexp::into_owned)
    }));
    owner.require_active().map_err(|error| error.to_string())?;
    let result = match result {
        Ok(value) => value.map_err(|error| error.to_string())?,
        Err(payload) => {
            if let Some(error) = payload.downcast_ref::<crate::sexp::context::RError>() {
                return Err(error.message.clone());
            }
            if let Some(crate::sexp::context::RSignal::Error { message }) =
                payload.downcast_ref::<crate::sexp::context::RSignal>()
            {
                return Err(message.clone());
            }
            std::panic::resume_unwind(payload)
        }
    };
    let mut calls = Vec::new();
    for index in 0..result.len() {
        let call = result
            .try_string_elt(index)
            .and_then(|value| value.try_as_string())
            .map_err(|error| error.to_string())?;
        owner.require_active().map_err(|error| error.to_string())?;
        calls.push(call);
    }
    Ok(calls.join("\n"))
}

#[cfg(test)]
mod tests {
    use crate::sexp::session::RSession;

    #[test]
    fn conditional_namespace_selection_is_session_local() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "rport-namespace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("NAMESPACE"),
            "if (getOption('rport.namespace.branch')) { export(selected) } else { export(other) }",
        )
        .unwrap();
        for (enabled, expected) in [(true, "selected"), (false, "other")] {
            let mut session = RSession::new_without_default_packages();
            session
                .eval_script_with_output_capture(&format!(
                    "options(rport.namespace.branch={})",
                    if enabled { "TRUE" } else { "FALSE" }
                ))
                .0
                .unwrap();
            for _ in 0..2 {
                session.with_active(|| {
                    let directives = super::super::read_namespace_directives(&directory)
                        .unwrap()
                        .unwrap();
                    assert_eq!(directives.exports, vec![expected]);
                });
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
