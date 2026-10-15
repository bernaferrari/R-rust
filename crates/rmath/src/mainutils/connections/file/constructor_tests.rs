//! Anonymous file status belongs to the original description, never its path.
use super::*;
use crate::sexp::{RSession, object::Sexp};
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{PairlistBuilder, SexpResult},
};
use std::{cell::Cell, rc::Rc};

struct DescriptionProvider {
    action: Rc<dyn Fn()>,
    calls: Rc<Cell<usize>>,
}

impl AltrepClass for DescriptionProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        let name = context.string("constructor.txt")?;
        self.calls.set(self.calls.get() + 1);
        (self.action)();
        Ok(AltrepElement::String(name))
    }
}

#[test]
fn constructor_roots_keep_detached_operands_through_real_provider_gc() {
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let factory = session.owner_token().unwrap().node_factory();
    let head: Rc<Cell<SEXP>> = Rc::new(Cell::new(ptr::null_mut()));
    let detached = head.clone();
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "connection.description.detach",
            DescriptionProvider {
                calls: calls.clone(),
                action: Rc::new(move || unsafe {
                    let head = detached.replace(ptr::null_mut());
                    if !head.is_null() {
                        SETCDR(head, R_NilValue());
                    }
                    crate::sexp::gengc::full_gc();
                }),
            },
        )
        .unwrap();
    let mut args = PairlistBuilder::from_factory(factory.clone());
    args.push(AltrepBuilder::new(class).build().unwrap(), None)
        .unwrap();
    args.push(factory.strings(&[""]).unwrap(), None).unwrap();
    args.push(factory.domain().logical(true), None).unwrap();
    args.push(factory.strings(&["UTF-8"]).unwrap(), None)
        .unwrap();
    args.push(factory.strings(&["default"]).unwrap(), None)
        .unwrap();
    args.push(factory.domain().logical(false), None).unwrap();
    let args = args.finish().unwrap();
    head.set(args.as_raw());
    let result = construct(&session, &args);
    assert_eq!(calls.get(), 1, "each string provider is evaluated once");
    assert!(args.try_cdr().unwrap().is_nil());
    let index = result.try_integer_elt(0).unwrap() as usize;
    session.with_active(|| {
        let table = connection_table();
        let conn = table[index].as_ref().unwrap();
        assert_eq!(conn.description, "constructor.txt");
        assert!(!conn.isopen);
    });
}

#[test]
fn constructor_roots_omitted_defaults_survive_every_allocation_gc() {
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let args = session.owner_token().unwrap().node_factory().nil();
    let collections = Rc::new(Cell::new(0));
    let observed = collections.clone();
    session.with_active_in(|instance| unsafe {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
    });
    let result = construct(&session, &args);
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 0;
    });
    assert!(collections.get() > 2);
    assert_eq!(result.typeof_(), SEXPTYPE::INTSXP);
    let index = result.try_integer_elt(0).unwrap() as usize;
    session.with_active(|| {
        let table = connection_table();
        let conn = table[index].as_ref().unwrap();
        assert!(conn.isopen && conn.canread && conn.canwrite);
    });
}

#[test]
fn constructor_roots_missing_defaults_survive_every_allocation_gc() {
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let f = session.owner_token().unwrap().node_factory();
    let mut args = PairlistBuilder::from_factory(f.clone());
    for _ in 0..6 {
        args.push(f.domain().missing(), None).unwrap();
    }
    let args = args.finish().unwrap();
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
    });
    let result = construct(&session, &args);
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 0;
    });
    assert_eq!(result.typeof_(), SEXPTYPE::INTSXP);
    session.with_active(|| {
        let table = connection_table();
        let conn = table[result.try_integer_elt(0).unwrap() as usize]
            .as_ref()
            .unwrap();
        assert!(conn.isopen && conn.canread && conn.canwrite);
    });
}

fn provider_exit(action: u8) {
    use std::cell::RefCell;
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let facade = Rc::new(RefCell::new(Some(session)));
    let replacement = Rc::new(RefCell::new(None));
    let original = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let f = original.node_factory().unwrap();
    let before = connection_table().iter().filter(|c| c.is_some()).count();
    let dropped = Rc::downgrade(&facade);
    let replaced = replacement.clone();
    let calls = Rc::new(Cell::new(0));
    let class = facade
        .borrow()
        .as_ref()
        .unwrap()
        .register_altrep_class(
            "connection.description.exit",
            DescriptionProvider {
                calls: calls.clone(),
                action: Rc::new(move || match action {
                    0 => std::panic::panic_any(681_u32),
                    1 => dropped
                        .upgrade()
                        .unwrap()
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .close(),
                    2 => {
                        drop(dropped.upgrade().unwrap().borrow_mut().take());
                        let mut new = RSession::new_for_gc_tests();
                        new.enable_browser_files();
                        *replaced.borrow_mut() = Some(new);
                    }
                    _ => unreachable!(),
                }),
            },
        )
        .unwrap()
        .into_owned()
        .unwrap();
    let mut args = PairlistBuilder::from_factory(f.clone());
    args.push(AltrepBuilder::new(class).build().unwrap(), None)
        .unwrap();
    let args = args.finish().unwrap();
    let pin = original.pin().unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            do_file(
                f.nil().as_raw(),
                f.nil().as_raw(),
                args.as_raw(),
                f.nil().as_raw(),
            )
        })
    }));
    assert_eq!(calls.get(), 1);
    let payload = outcome.expect_err("provider exit cannot publish a connection");
    if action == 0 {
        assert_eq!(*payload.downcast::<u32>().unwrap(), 681);
        facade.borrow().as_ref().unwrap().with_active(|| {
            assert_eq!(
                connection_table().iter().filter(|c| c.is_some()).count(),
                before
            );
        });
    } else {
        assert_eq!(
            payload.downcast_ref::<RError>().unwrap().message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        );
        assert!(original.pin().is_err());
    }
    if let Some(new) = replacement.borrow().as_ref() {
        new.with_active(|| {
            assert_eq!(
                connection_table().iter().filter(|c| c.is_some()).count(),
                before
            )
        });
    }
}

#[test]
fn constructor_roots_preserve_provider_unwind_without_publication() {
    provider_exit(0);
}
#[test]
fn constructor_roots_refuse_closed_original_provider() {
    provider_exit(1);
}
#[test]
fn constructor_roots_refuse_dropped_original_with_replacement_active() {
    provider_exit(2);
}

#[test]
fn constructor_roots_keep_encoding_through_collecting_logical_coercions() {
    struct LogicalProvider {
        head: Rc<Cell<SEXP>>,
        value: i32,
        calls: Rc<Cell<usize>>,
    }
    impl AltrepClass for LogicalProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            self.calls.set(self.calls.get() + 1);
            let head = self.head.replace(ptr::null_mut());
            if !head.is_null() {
                unsafe {
                    SETCDR(head, R_NilValue());
                }
            }
            crate::sexp::gengc::full_gc();
            Ok(AltrepElement::Integer(self.value))
        }
    }
    let mut session = RSession::new_for_gc_tests();
    session.enable_browser_files();
    let f = session.owner_token().unwrap().node_factory();
    let head: Rc<Cell<SEXP>> = Rc::new(Cell::new(ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let provider = |name, value| {
        let class = session
            .register_altrep_class(
                name,
                LogicalProvider {
                    head: head.clone(),
                    value,
                    calls: calls.clone(),
                },
            )
            .unwrap();
        AltrepBuilder::new(class).build().unwrap()
    };
    let mut args = PairlistBuilder::from_factory(f.clone());
    args.push(f.strings(&["constructor.txt"]).unwrap(), None)
        .unwrap();
    args.push(f.strings(&[""]).unwrap(), None).unwrap();
    let block = args
        .push_cell(provider("constructor.blocking", 1), None)
        .unwrap();
    args.push(f.strings(&["UTF-8"]).unwrap(), None).unwrap();
    args.push(f.strings(&["default"]).unwrap(), None).unwrap();
    args.push(provider("constructor.raw", 0), None).unwrap();
    let args = args.finish().unwrap();
    head.set(block.as_raw());
    let result = construct(&session, &args);
    assert_eq!(calls.get(), 2);
    assert!(block.try_cdr().unwrap().is_nil());
    session.with_active(|| {
        let table = connection_table();
        let conn = table[result.try_integer_elt(0).unwrap() as usize]
            .as_ref()
            .unwrap();
        assert!(!conn.isopen && conn.canseek);
    });
}

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
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            do_file(
                ptr::null_mut(),
                ptr::null_mut(),
                arguments.as_raw(),
                ptr::null_mut(),
            )
        }));
        let raw = match outcome {
            Ok(raw) => raw,
            Err(payload) => {
                eprintln!(
                    "constructor failure: {:?}",
                    payload.downcast_ref::<RError>().map(|e| &e.message)
                );
                std::panic::resume_unwind(payload)
            }
        };
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
