//! Anonymous file status belongs to the original description, never its path.
use super::*;
use crate::sexp::{RSession, object::Sexp};

fn arguments(session: &RSession, description: &str, open: &str) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil();
    let values = [
        factory.strings(&[description]).unwrap(),
        factory.strings(&[open]).unwrap(),
        factory.domain().logical(true),
        factory.strings(&["native.enc"]).unwrap(),
        factory.strings(&["default"]).unwrap(),
        factory.domain().logical(false),
    ];
    let mut arguments = nil.clone();
    for value in values.iter().rev() {
        arguments = factory.pairlist_cell(value, &arguments, &nil).unwrap();
    }
    arguments.into_owned().unwrap()
}

fn construct(session: &RSession, arguments: &Sexp<'_>) -> Sexp<'static> {
    session.with_active(|| unsafe {
        let raw = do_file(
            ptr::null_mut(),
            ptr::null_mut(),
            arguments.as_raw(),
            ptr::null_mut(),
        );
        session
            .owner_token()
            .unwrap()
            .sexp(raw)
            .unwrap()
            .into_owned()
            .unwrap()
    })
}

#[test]
fn named_pid_prefixed_host_file_stays_closed_and_preserves_contents() {
    let session = RSession::new_for_gc_tests();
    let path = std::env::temp_dir().join(format!("Rf{}rport-named-connection", std::process::id()));
    std::fs::write(&path, b"original\n").unwrap();
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _remove = Remove(path.clone());
    let args = arguments(&session, path.to_str().unwrap(), "");
    let connection = construct(&session, &args);
    let index = connection.try_integer_elt(0).unwrap() as usize;
    session.with_active(|| {
        let table = connection_table();
        let connection = table[index].as_ref().unwrap();
        assert!(
            !connection.isopen,
            "a nonempty path is a deferred named connection"
        );
        assert_eq!(connection.mode, "r");
    });
    assert_eq!(std::fs::read(path).unwrap(), b"original\n");
}

#[test]
fn named_browser_file_is_deferred_without_creating_a_host_file() {
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let args = arguments(&session, "named.txt", "");
    let connection = construct(&session, &args);
    let index = connection.try_integer_elt(0).unwrap() as usize;
    session.with_active(|| {
        let table = connection_table();
        let connection = table[index].as_ref().unwrap();
        assert!(matches!(connection.kind, ConnKind::BrowserFile));
        assert!(!connection.isopen);
        assert_eq!(connection.description, "named.txt");
    });
    assert!(session.list_browser_files().is_empty());
}

#[test]
fn anonymous_browser_file_is_owned_open_read_write_and_recovers_after_budget_rejection() {
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let args = arguments(&session, "", "");
    let connection = construct(&session, &args);
    let index = connection.try_integer_elt(0).unwrap() as usize;
    session.with_active(|| {
        let table = connection_table();
        let connection = table[index].as_ref().unwrap();
        assert!(matches!(connection.kind, ConnKind::AnonymousFile(_)));
        assert!(
            connection.isopen && connection.canread && connection.canwrite && connection.canseek
        );
        assert_eq!(connection.description, "");
        assert_eq!(connection.mode, "w+");
    });
    session.with_active_in(|instance| unsafe {
        let arena = &mut (*instance).arena;
        arena.set_budget(crate::sexp::memory::ArenaBudget::new(
            arena.total_bytes_allocated() + 3,
            0,
        ));
    });
    session.with_active(|| connection_write_bytes(index as c_int, &[1, 2, 3]));
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session.with_active(|| connection_write_bytes(index as c_int, &[4]));
    }))
    .expect_err("replacement capacity and original storage must both be admitted");
    assert!(
        error
            .downcast_ref::<RError>()
            .unwrap()
            .message
            .contains("session memory limit")
    );
    session.with_active_in(|instance| unsafe {
        (*instance)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::unlimited());
    });
    session.with_active(|| {
        crate::sexp::gengc::full_gc();
        assert_eq!(connection_read_all(index as c_int), [1, 2, 3]);
        connection_table()[index] = None;
    });
    assert!(session.list_browser_files().is_empty());
    let recovered = session.eval_code_with_output_capture("1+1").0.unwrap();
    assert_eq!(recovered.try_real_elt(0).unwrap(), 2.0);
}
