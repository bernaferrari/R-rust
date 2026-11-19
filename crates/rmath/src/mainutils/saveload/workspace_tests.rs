use super::*;
use crate::sexp::{
    envir::{R_findVarInFrame, defineVar, lock_binding_raw, unlock_binding_raw},
    globals::{R_EmptyEnv, R_GlobalEnv},
    memory::ArenaBudget,
    memory_ext::NewEnvironment,
    session::RSession,
};
use workspace_file::SaveOptions;

fn browser_session() -> RSession {
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    // Miri remains isolated from the native wall clock.
    session.with_active_in(|owner| unsafe { (*owner).browser_files.set_test_clock(1.0) });
    session
}

fn options() -> SaveOptions {
    SaveOptions {
        version: 3,
        ascii: false,
        codec: None,
        eval_promises: true,
    }
}

fn filename<'a>(
    owner: crate::sexp::owner::OwnerToken<'a>,
    path: &str,
) -> crate::sexp::object::Sexp<'a> {
    owner.node_factory().strings(&[path]).unwrap()
}

fn expect_error(operation: impl FnOnce()) -> String {
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).unwrap_err();
    error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message
}

#[test]
fn workspace_save_denial_preserves_admitted_browser_file() {
    let mut session = browser_session();
    session
        .put_browser_file("workspace.RData", b"prior workspace")
        .unwrap();
    session.with_active(|| unsafe {
        let owner = workspace_owner();
        let envir = R_GlobalEnv();
        let name = Rf_install(c"workspace_value".as_ptr());
        defineVar(name, crate::sexp::constructors::Rf_ScalarInteger(17), envir);
        let file = filename(owner, "workspace.RData");
        let baseline = owner
            .with_arena(|arena| arena.total_bytes_allocated() + arena.workspace_bytes())
            .unwrap();
        let limit = baseline + 32 * 1024;
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let mut opts = options();
        opts.codec = Some("gzip");
        assert!(
            expect_error(|| {
                workspace_file::save(
                    owner,
                    &["workspace_value".into()],
                    file.as_raw(),
                    envir,
                    opts,
                );
            })
            .contains("session memory limit")
        );
        assert_eq!(
            owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
            limit
        );
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
            .unwrap();
    });
    assert_eq!(
        session.get_browser_file("workspace.RData").unwrap(),
        b"prior workspace"
    );
}

#[test]
fn workspace_save_of_unsupported_value_does_not_replace_file() {
    let mut session = browser_session();
    session
        .put_browser_file("workspace.RData", b"prior workspace")
        .unwrap();
    session.with_active(|| unsafe {
        let owner = workspace_owner();
        let envir = R_GlobalEnv();
        let name = Rf_install(c"workspace_value".as_ptr());
        let unsupported = crate::sexp::memory_ext::allocSExp(SEXPTYPE::WEAKREFSXP);
        defineVar(name, unsupported, envir);
        let file = filename(owner, "workspace.RData");
        assert!(
            expect_error(|| {
                workspace_file::save(
                    owner,
                    &["workspace_value".into()],
                    file.as_raw(),
                    envir,
                    options(),
                );
            })
            .contains("unsupported workspace object")
        );
    });
    assert_eq!(
        session.get_browser_file("workspace.RData").unwrap(),
        b"prior workspace"
    );
}

#[test]
fn workspace_restore_is_unchanged_after_a_locked_binding_or_truncated_file() {
    let session = browser_session();
    session.with_active(|| unsafe {
        let owner = workspace_owner();
        let factory = owner.node_factory();
        let src = R_GlobalEnv();
        let a = Rf_install(c"workspace_a".as_ptr());
        let b = Rf_install(c"workspace_b".as_ptr());
        let target = factory
            .wrap(NewEnvironment(R_NilValue(), R_EmptyEnv(), R_NilValue()))
            .unwrap();
        defineVar(a, crate::sexp::constructors::Rf_ScalarInteger(1), src);
        defineVar(b, crate::sexp::constructors::Rf_ScalarInteger(2), src);
        defineVar(
            a,
            crate::sexp::constructors::Rf_ScalarInteger(99),
            target.as_raw(),
        );
        lock_binding_raw(target.as_raw(), a);
        let file = filename(owner, "workspace.RData");
        let names = [
            "workspace_a".into(),
            "workspace_b".into(),
            "workspace_a".into(),
        ];
        workspace_file::save(owner, &names, file.as_raw(), src, options());
        let data = crate::mainutils::browser_files::read_current("workspace.RData").unwrap();
        assert!(data.starts_with(b"RDX3\nX\n"));
        assert!(
            expect_error(|| {
                workspace_file::load(owner, file.as_raw(), target.as_raw());
            })
            .contains("locked binding")
        );
        assert_eq!(R_findVarInFrame(target.as_raw(), b), R_UnboundValue());
        assert_eq!(*INTEGER(R_findVarInFrame(target.as_raw(), a)), 99);

        unlock_binding_raw(target.as_raw(), a);
        crate::mainutils::browser_files::write_current("workspace.RData", &data[..data.len() - 1])
            .unwrap();
        expect_error(|| {
            workspace_file::load(owner, file.as_raw(), target.as_raw());
        });
        assert_eq!(R_findVarInFrame(target.as_raw(), b), R_UnboundValue());
        assert_eq!(*INTEGER(R_findVarInFrame(target.as_raw(), a)), 99);

        crate::mainutils::browser_files::write_current("workspace.RData", &data).unwrap();
        let loaded = workspace_file::load(owner, file.as_raw(), target.as_raw());
        assert_eq!(XLENGTH(loaded), 3);
        assert_eq!(STRING_ELT(loaded, 0), STRING_ELT(loaded, 2));
        assert_eq!(*INTEGER(R_findVarInFrame(target.as_raw(), a)), 1);
        assert_eq!(*INTEGER(R_findVarInFrame(target.as_raw(), b)), 2);
    });
}

#[test]
fn legacy_workspace_loading_retains_old_objects_and_checks_lengths() {
    let mut session = browser_session();
    session
        .put_browser_file("previous.RData", b"RDA3\n1\n1 x\n13\n1\n42\n0\n")
        .unwrap();
    session
        .put_browser_file("truncated.RData", b"RDA3\n1\n1 x\n13\n-1\n42\n0\n")
        .unwrap();
    session.with_active(|| unsafe {
        let owner = workspace_owner();
        let factory = owner.node_factory();
        let target = factory
            .wrap(NewEnvironment(R_NilValue(), R_EmptyEnv(), R_NilValue()))
            .unwrap();
        let previous = filename(owner, "previous.RData");
        let truncated = filename(owner, "truncated.RData");
        let name = Rf_install(c"x".as_ptr());
        assert_eq!(
            XLENGTH(workspace_file::load(
                owner,
                previous.as_raw(),
                target.as_raw()
            )),
            1
        );
        assert_eq!(*INTEGER(R_findVarInFrame(target.as_raw(), name)), 42);
        assert!(
            expect_error(|| {
                workspace_file::load(owner, truncated.as_raw(), target.as_raw());
            })
            .contains("invalid length")
        );
        assert_eq!(*INTEGER(R_findVarInFrame(target.as_raw(), name)), 42);
    });
}
