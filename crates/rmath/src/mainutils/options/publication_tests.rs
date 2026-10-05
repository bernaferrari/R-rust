//! The private options publisher must preserve the public base binding lock.
use super::*;

#[test]
fn owned_options_publication_preserves_locked_binding_and_base_projection() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        InitOptions();
        let factory = session.owner_token().unwrap().node_factory();
        let symbol = options_symbol();
        let base = R_BaseEnv();
        crate::sexp::envir::lock_binding_raw(base, symbol);
        assert!(crate::sexp::envir::binding_is_locked_raw(base, symbol));
        let value = factory.strings(&["private option"]).unwrap();
        SetOptionByName("owned_locked_option", value.as_raw());
        assert!(crate::sexp::envir::binding_is_locked_raw(base, symbol));
        let option = factory
            .wrap(GetOptionByName("owned_locked_option"))
            .unwrap();
        assert_eq!(
            option.try_string_value_elt(0).unwrap().as_deref(),
            Some("private option")
        );
        let snapshot = factory
            .wrap(crate::sexp::envir::R_findVarInFrame(base, symbol))
            .unwrap();
        let mut cell = snapshot;
        let mut found = false;
        while !cell.is_nil() {
            if cell.try_tag_name_eq(b"owned_locked_option").unwrap() {
                assert_eq!(cell.try_car().unwrap(), option);
                found = true;
                break;
            }
            cell = cell.try_cdr().unwrap();
        }
        assert!(
            found,
            "base snapshot must publish the original option value"
        );
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            defineVar(symbol, factory.nil().as_raw(), base);
        }));
        assert!(
            rejected.is_err(),
            "public assignment must still honor the binding lock"
        );
        assert!(crate::sexp::envir::binding_is_locked_raw(base, symbol));
        SetOptionByName("owned_locked_option", factory.nil().as_raw());
        assert!(
            factory
                .wrap(GetOptionByName("owned_locked_option"))
                .unwrap()
                .is_nil()
        );
        assert!(crate::sexp::envir::binding_is_locked_raw(base, symbol));
    });
}

fn binding_cell(base: &Sexp<'_>) -> Sexp<'static> {
    let mut cell = base.try_frame().unwrap().into_owned().unwrap();
    while !cell.is_nil() {
        if cell.try_tag_name_eq(b".Options").unwrap() {
            return cell;
        }
        cell = cell.try_cdr().unwrap().into_owned().unwrap();
    }
    panic!("missing actual options binding");
}

#[test]
fn owned_options_publication_resolves_frame_replaced_by_collecting_callback() {
    use std::{cell::Cell, rc::Rc};
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active_in(|instance| unsafe {
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        (*instance).options_initialization = OptionsInitialization::Initialized;
        let value = f.strings(&["selected"]).unwrap().into_owned().unwrap();
        (*instance).options.insert("selected".into(), value);
        let _warm = f.wrap(Rf_install(c"selected".as_ptr())).unwrap();
        refresh_options_binding(owner);
        let base = f.wrap(R_BaseEnv()).unwrap().into_owned().unwrap();
        let symbol = f.wrap(options_symbol()).unwrap();
        crate::sexp::envir::lock_binding_raw(base.as_raw(), symbol.as_raw());
        let previous = binding_cell(&base);
        let old_snapshot = previous.try_car().unwrap().into_owned().unwrap();
        let replacement = f
            .pairlist_cell(&f.nil(), &f.nil(), &symbol)
            .unwrap()
            .into_owned()
            .unwrap();
        let fired = Rc::new(Cell::new(false));
        let observed = fired.clone();
        let captured = replacement.clone();
        let base_root = base.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if !observed.replace(true) {
                SET_FRAME(base_root.as_raw(), captured.as_raw());
                crate::sexp::gengc::full_gc_in(instance);
            }
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        refresh_options_binding(owner);
        assert!(
            fired.get(),
            "real options construction must run the callback"
        );
        assert_eq!(base.try_frame().unwrap(), replacement);
        assert_eq!(
            previous.try_car().unwrap(),
            old_snapshot,
            "detached cell must not receive publication"
        );
        let snapshot = replacement.try_car().unwrap();
        assert!(snapshot.try_tag_name_eq(b"selected").unwrap());
        assert_eq!(
            snapshot
                .try_car()
                .unwrap()
                .try_string_value_elt(0)
                .unwrap()
                .as_deref(),
            Some("selected")
        );
        assert!(crate::sexp::envir::binding_is_locked_raw(
            base.as_raw(),
            symbol.as_raw()
        ));
    });
}

fn callback_panic(revoke: bool) {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(
        crate::sexp::session::RSession::new_for_gc_tests(),
    )));
    let weak = Rc::downgrade(&facade);
    let owner = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = owner.pin().unwrap();
    let fired = Rc::new(Cell::new(false));
    let observed = fired.clone();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            let token = OwnerToken::current().unwrap();
            let f = token.node_factory();
            (*pin.as_ptr()).options_initialization = OptionsInitialization::Initialized;
            (*pin.as_ptr()).options.insert(
                "selected".into(),
                f.strings(&["selected"]).unwrap().into_owned().unwrap(),
            );
            let _warm = f.wrap(Rf_install(c"selected".as_ptr())).unwrap();
            refresh_options_binding(token);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    if revoke {
                        weak.upgrade()
                            .unwrap()
                            .borrow_mut()
                            .as_mut()
                            .unwrap()
                            .close();
                    }
                    std::panic::panic_any(317_u32);
                }
            }));
            (*pin.as_ptr()).memory_state.gc_force_gap = 1;
            (*pin.as_ptr()).memory_state.gc_force_wait = 1;
            refresh_options_binding(token);
        });
    }));
    assert!(fired.get());
    let payload = outcome.expect_err("callback must unwind");
    if revoke {
        let error = payload.downcast::<RError>().unwrap();
        assert_eq!(
            error.message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        );
        assert!(owner.pin().is_err());
    } else {
        assert_eq!(*payload.downcast::<u32>().unwrap(), 317);
        assert!(owner.pin().is_ok());
        unsafe {
            crate::sexp::session::with_instance_active(pin.as_ptr(), || {
                (*pin.as_ptr()).memory_state.gc_force_gap = 0;
                (*pin.as_ptr()).memory_state.gc_force_wait = 0;
                let token = OwnerToken::current().unwrap();
                refresh_options_binding(token);
                assert!(!GetOptionByName("selected").is_null());
            });
        }
    }
    assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
}

#[test]
fn owned_options_publication_preserves_live_callback_panic_and_recovers() {
    callback_panic(false);
}

#[test]
fn owned_options_publication_rejects_revoked_callback_panic() {
    callback_panic(true);
}

#[test]
fn owned_options_publication_rejects_foreign_snapshot_before_mutation() {
    let original = crate::sexp::session::RSession::new_for_gc_tests();
    let other = crate::sexp::session::RSession::new_for_gc_tests();
    let foreign = other.with_active(|| {
        other
            .owner_token()
            .unwrap()
            .node_factory()
            .strings(&["foreign"])
            .unwrap()
            .into_owned()
            .unwrap()
    });
    original.with_active(|| unsafe {
        InitOptions();
        let f = original.owner_token().unwrap().node_factory();
        let base = f.wrap(R_BaseEnv()).unwrap();
        let symbol = f.wrap(options_symbol()).unwrap();
        let before = binding_cell(&base).try_car().unwrap();
        assert!(publication::replace_existing(&f, &base, &symbol, &foreign).is_err());
        assert_eq!(binding_cell(&base).try_car().unwrap(), before);
    });
}
