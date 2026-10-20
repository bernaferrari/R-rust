use super::*;
use crate::sexp::{
    RSession, Sexp,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::SexpResult,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct FilenameProvider(Rc<dyn Fn()>);
impl AltrepClass for FilenameProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        (self.0)();
        Ok(AltrepElement::String(context.string("binary/input.bin")?))
    }
}

struct NumericProvider {
    calls: Rc<Cell<usize>>,
    length: i64,
    kind: SEXPTYPE,
}

struct RawProvider(Rc<dyn Fn()>);
impl AltrepClass for RawProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::RAWSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(4)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, index: i64) -> SexpResult<AltrepElement<'s>> {
        (self.0)();
        Ok(AltrepElement::Raw(index as u8 + 1))
    }
}

struct ConnectionProvider {
    kind: SEXPTYPE,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for ConnectionProvider {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        (self.callback)();
        Ok(if self.kind == SEXPTYPE::INTSXP {
            AltrepElement::Integer(1)
        } else {
            AltrepElement::String(context.string("data")?)
        })
    }
}

fn replace_connection(index: usize) {
    let mut replacement = RConn::new(
        "rawConnection",
        "replacement",
        "w+b",
        ConnKind::RawConnection,
    );
    replacement.isopen = true;
    replacement.text = false;
    replacement.canread = true;
    replacement.canwrite = true;
    replacement.raw_data = vec![71, 72];
    connection_table()[index] = Some(Box::new(replacement));
}

fn assert_replacement_untouched(session: &RSession, index: usize) {
    session.with_active(|| {
        let table = connection_table();
        let replacement = table[index].as_ref().unwrap();
        assert_eq!(replacement.raw_data, [71, 72]);
        assert_eq!(replacement.raw_pos, 0);
    });
}

#[test]
fn owned_binary_connection_token_rejects_slot_reuse_from_size_and_character_providers() {
    for kind in [SEXPTYPE::INTSXP, SEXPTYPE::STRSXP] {
        let mut session = session();
        let con = session
            .eval_script_with_output_capture("file('', 'w+b')")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let index = con.try_integer_elt(0).unwrap() as usize;
        let calls = Rc::new(Cell::new(0));
        let callback_calls = calls.clone();
        let class = session
            .register_altrep_class(
                "binary.connection.replace",
                ConnectionProvider {
                    kind,
                    callback: Rc::new(move || {
                        callback_calls.set(callback_calls.get() + 1);
                        replace_connection(index);
                        crate::sexp::gengc::full_gc();
                    }),
                },
            )
            .unwrap();
        let provider = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let values = if kind == SEXPTYPE::INTSXP {
            let object = session
                .owner_token()
                .unwrap()
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap()
                .into_owned()
                .unwrap();
            vec![object, con, provider]
        } else {
            vec![provider, con]
        };
        let error = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            filename_owned::write(arguments(&session, &values))
        })) {
            Ok(result) => result.unwrap_err(),
            Err(payload) => payload.downcast::<RError>().unwrap().message,
        };
        assert!(
            error.contains("original connection is no longer available"),
            "{error}"
        );
        assert_eq!(calls.get(), 1);
        assert_replacement_untouched(&session, index);
        assert_eq!(
            session
                .eval_code_with_output_capture("1+1")
                .0
                .unwrap()
                .try_real_elt(0)
                .unwrap(),
            2.0
        );
    }
}

#[test]
fn owned_binary_connection_token_rejects_slot_reuse_from_count_provider_without_reading_replacement()
 {
    let mut session = session();
    let con = session
        .eval_script_with_output_capture("file('', 'w+b')")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let index = con.try_integer_elt(0).unwrap() as usize;
    let class = session
        .register_altrep_class(
            "binary.connection.read.replace",
            ConnectionProvider {
                kind: SEXPTYPE::INTSXP,
                callback: Rc::new(move || {
                    replace_connection(index);
                    crate::sexp::gengc::full_gc();
                }),
            },
        )
        .unwrap();
    let count = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let error = filename_owned::read(read_arguments(&session, con, Some(count))).unwrap_err();
    assert!(
        error.contains("original connection is no longer available"),
        "{error}"
    );
    assert_replacement_untouched(&session, index);
}

#[test]
fn owned_anonymous_write_lines_token_rejects_slot_reuse_from_character_provider() {
    let mut session = session();
    let con = session
        .eval_script_with_output_capture("file('')")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let index = con.try_integer_elt(0).unwrap() as usize;
    let class = session
        .register_altrep_class(
            "anonymous.lines.replace",
            ConnectionProvider {
                kind: SEXPTYPE::STRSXP,
                callback: Rc::new(move || {
                    replace_connection(index);
                    crate::sexp::gengc::full_gc();
                }),
            },
        )
        .unwrap();
    let text = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let separator = session
        .owner_token()
        .unwrap()
        .node_factory()
        .strings(&["\n"])
        .unwrap()
        .into_owned()
        .unwrap();
    let use_bytes = session
        .owner_token()
        .unwrap()
        .node_factory()
        .domain()
        .logical(false)
        .into_owned()
        .unwrap();
    let args = arguments(&session, &[text, con, separator, use_bytes]);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session.with_active(|| unsafe {
            do_writeLines(
                ptr::null_mut(),
                ptr::null_mut(),
                args.as_raw(),
                ptr::null_mut(),
            )
        })
    }))
    .expect_err("a replaced connection must not receive lines");
    let error = &payload.downcast_ref::<RError>().unwrap().message;
    assert!(
        error.contains("original connection is no longer available"),
        "{error}"
    );
    assert_replacement_untouched(&session, index);
}

#[test]
fn owned_anonymous_seek_token_rejects_slot_reuse_from_each_parameter_provider() {
    for position in 1..=3 {
        let mut session = session();
        let con = session
            .eval_script_with_output_capture("file('', 'w+b')")
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let index = con.try_integer_elt(0).unwrap() as usize;
        let class = session
            .register_altrep_class(
                "anonymous.seek.replace",
                ConnectionProvider {
                    kind: SEXPTYPE::INTSXP,
                    callback: Rc::new(move || {
                        replace_connection(index);
                        crate::sexp::gengc::full_gc();
                    }),
                },
            )
            .unwrap();
        let provider = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        let zero = session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap()
            .into_owned()
            .unwrap();
        let mut values = [con, zero.clone(), zero.clone(), zero];
        values[position] = provider;
        let args = arguments(&session, &values);
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.with_active(|| unsafe {
                do_seek(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    args.as_raw(),
                    ptr::null_mut(),
                )
            })
        }))
        .expect_err("a replaced connection must not be sought");
        let error = &payload.downcast_ref::<RError>().unwrap().message;
        assert_eq!(error, "original connection is no longer available");
        assert_replacement_untouched(&session, index);
        assert_eq!(
            session
                .eval_code_with_output_capture("1+1")
                .0
                .unwrap()
                .try_real_elt(0)
                .unwrap(),
            2.0
        );
    }
}
impl AltrepClass for NumericProvider {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        Ok(if self.kind == SEXPTYPE::INTSXP {
            AltrepElement::Integer(1)
        } else {
            AltrepElement::Real(1.0)
        })
    }
}

fn session() -> RSession {
    let mut session = RSession::new_for_gc_tests();
    session
        .put_browser_file("binary/input.bin", &[9, 8, 7, 6])
        .unwrap();
    session
}

fn arguments(session: &RSession, values: &[Sexp<'static>]) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil();
    let mut args = nil.clone();
    for value in values.iter().rev() {
        args = factory.pairlist_cell(value, &args, &nil).unwrap();
    }
    args.into_owned().unwrap()
}

fn read_arguments(
    session: &RSession,
    filename: Sexp<'static>,
    count: Option<Sexp<'static>>,
) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    let what = factory.strings(&["raw"]).unwrap().into_owned().unwrap();
    let mut values = vec![filename, what];
    if let Some(count) = count {
        values.push(count);
    }
    arguments(session, &values)
}

fn filename(session: &RSession) -> Sexp<'static> {
    session
        .owner_token()
        .unwrap()
        .node_factory()
        .strings(&["binary/input.bin"])
        .unwrap()
        .into_owned()
        .unwrap()
}

fn budget_headroom(session: &RSession, bytes: usize) {
    session.with_active_in(|instance| unsafe {
        let arena = &mut (*instance).arena;
        arena.set_budget(crate::sexp::memory::ArenaBudget::new(
            arena.total_bytes_allocated() + bytes,
            0,
        ));
    });
}

#[test]
fn owned_binary_generic_sources_retain_admission_until_consumer_drop() {
    let mut session = session();
    let factory = session.owner_token().unwrap().node_factory();
    let raw = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 4)))
        .unwrap()
        .into_owned()
        .unwrap();
    let connection = session
        .eval_script_with_output_capture("rawConnection(as.raw(c(1,2,3,4)), 'rb')")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let owner = raw.runtime_owner.as_ref().unwrap().clone();
    for (source, admitted) in [(filename(&session), 4), (raw, 4), (connection, 7)] {
        budget_headroom(&session, admitted);
        let buffer = filename_owned::source_bytes(source.clone(), Some(4)).unwrap();
        assert_eq!(buffer.len(), 4);
        crate::sexp::owner::with_runtime(&owner, |access| {
            assert!(filename_owned::reserve(access, 1).is_err());
        })
        .unwrap();
        drop(buffer);
        crate::sexp::owner::with_runtime(&owner, |access| {
            assert!(filename_owned::reserve(access, admitted).is_ok());
        })
        .unwrap();
    }
}

#[test]
fn owned_binary_generic_decoder_keeps_source_and_workspace_admitted_together() {
    let session = session();
    let source = filename(&session);
    let parameters = FilenameReadParameters {
        kind: BinaryKind::Integer,
        count: 1,
        size: 4,
        signed: true,
        order: ByteOrder::Little,
    };
    budget_headroom(&session, 7);
    let buffer = filename_owned::source_bytes(source.clone(), Some(4)).unwrap();
    assert!(
        filename_owned::decode_source(source.clone(), &parameters, &buffer)
            .unwrap_err()
            .contains("memory limit")
    );
    drop(buffer);
    session.with_active_in(|instance| unsafe {
        (*instance)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::unlimited());
    });
    let buffer = filename_owned::source_bytes(source.clone(), Some(4)).unwrap();
    let result = filename_owned::decode_source(source, &parameters, &buffer).unwrap();
    assert_eq!(result.try_integer_elt(0).unwrap(), 0x06070809);
}

#[test]
fn owned_binary_generic_raw_admits_before_provider_and_recovers_from_collecting_panic() {
    let session = session();
    let calls = Rc::new(Cell::new(0));
    let panic_next = Rc::new(Cell::new(true));
    let callback_calls = calls.clone();
    let callback_panic = panic_next.clone();
    let class = session
        .register_altrep_class(
            "binary.raw.gc",
            RawProvider(Rc::new(move || {
                callback_calls.set(callback_calls.get() + 1);
                crate::sexp::gengc::full_gc();
                if callback_panic.replace(false) {
                    std::panic::panic_any(734_u32);
                }
            })),
        )
        .unwrap();
    let source = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    budget_headroom(&session, 3);
    assert!(filename_owned::source_bytes(source.clone(), Some(4)).is_err());
    assert_eq!(calls.get(), 0);
    budget_headroom(&session, 4);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        filename_owned::source_bytes(source.clone(), Some(4))
    }))
    .err()
    .expect("exact provider panic");
    assert_eq!(*payload.downcast::<u32>().unwrap(), 734);
    budget_headroom(&session, 4);
    let buffer = filename_owned::source_bytes(source, Some(4)).unwrap();
    assert_eq!(&*buffer, &[1, 2, 3, 4]);
    assert_eq!(calls.get(), 5);
    // The admission tokens own their ledger, so late buffer destruction
    // does not dereference the destroyed session or re-enter its arena.
    drop(session);
    drop(buffer);
}

#[test]
fn owned_binary_filename_selected_children_survive_provider_detachment_and_full_gc() {
    let session = session();
    let identity = Rc::new(RefCell::new(None::<crate::sexp::heap::CheckedNode>));
    let observed = Rc::new(Cell::new(0));
    let callback_identity = identity.clone();
    let callback_observed = observed.clone();
    let owner = filename(&session).runtime_owner.as_ref().unwrap().clone();
    let domain = crate::sexp::owner::with_runtime(&owner, |access| access.domain()).unwrap();
    let class = session
        .register_altrep_class(
            "binary.filename.gc",
            FilenameProvider(Rc::new(move || {
                callback_observed.set(callback_observed.get() + 1);
                let allocation = callback_identity.borrow().as_ref().unwrap().clone();
                let raw = allocation
                    .heap_identity()
                    .projection_of_link(allocation.link().unwrap())
                    .unwrap();
                let value = domain.wrap(raw).unwrap();
                unsafe {
                    SETCAR(value.as_raw(), domain.nil().as_raw());
                    SETCDR(value.as_raw(), domain.nil().as_raw());
                }
                drop(value);
                crate::sexp::gengc::full_gc();
            })),
        )
        .unwrap();
    let con = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let args = read_arguments(&session, con, None);
    *identity.borrow_mut() = Some(args.allocation().unwrap().clone());
    let result = filename_owned::read(args.clone()).unwrap();
    assert_eq!(observed.get(), 1);
    assert!(args.try_car().unwrap().is_nil());
    assert!(args.try_cdr().unwrap().is_nil());
    assert_eq!(result.try_raw_elt(0).unwrap(), 9);
}

#[test]
fn owned_binary_filename_live_panic_is_exact_and_operation_recovers() {
    let session = session();
    let panic_next = Rc::new(Cell::new(true));
    let callback_next = panic_next.clone();
    let class = session
        .register_altrep_class(
            "binary.filename.panic",
            FilenameProvider(Rc::new(move || {
                if callback_next.replace(false) {
                    std::panic::panic_any(731_u32);
                }
            })),
        )
        .unwrap();
    let con = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let args = read_arguments(&session, con, None);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        filename_owned::read(args.clone())
    }))
    .unwrap_err();
    assert_eq!(*payload.downcast::<u32>().unwrap(), 731);
    assert_eq!(
        filename_owned::read(args).unwrap().try_raw_elt(0).unwrap(),
        9
    );
}

#[test]
fn owned_binary_filename_revocation_rejects_original_provider_result() {
    let session = session();
    let owner = filename(&session).runtime_owner.as_ref().unwrap().clone();
    let callback_owner = owner.clone();
    let class = session
        .register_altrep_class(
            "binary.filename.close",
            FilenameProvider(Rc::new(move || {
                let pin = callback_owner.pin().unwrap();
                unsafe { crate::sexp::instance::revoke_instance_availability(pin.as_ptr()) };
                std::panic::panic_any(732_u32);
            })),
        )
        .unwrap();
    let con = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let args = read_arguments(&session, con, None);
    assert!(filename_owned::read(args).is_err());
    assert!(owner.pin().is_err());
}

#[test]
fn owned_binary_filename_read_admits_before_count_provider_and_releases_on_error() {
    let mut session = session();
    session
        .put_browser_file("binary/input.bin", &[9; 8192])
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "binary.count.budget",
            NumericProvider {
                calls: calls.clone(),
                length: 1,
                kind: SEXPTYPE::INTSXP,
            },
        )
        .unwrap();
    let count = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let args = read_arguments(&session, filename(&session), Some(count));
    session.with_active_in(|instance| unsafe {
        let arena = &mut (*instance).arena;
        arena.set_budget(crate::sexp::memory::ArenaBudget::new(
            arena.total_bytes_allocated() + 4096,
            0,
        ));
    });
    assert!(
        filename_owned::read(args.clone())
            .unwrap_err()
            .contains("memory limit")
    );
    assert_eq!(calls.get(), 0);
    session.with_active_in(|instance| unsafe {
        (*instance)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::unlimited());
    });
    assert_eq!(
        filename_owned::read(args).unwrap().try_raw_elt(0).unwrap(),
        9
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn owned_binary_filename_write_admits_before_data_provider_and_sink() {
    let mut session = session();
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "binary.object.budget",
            NumericProvider {
                calls: calls.clone(),
                length: 128,
                kind: SEXPTYPE::REALSXP,
            },
        )
        .unwrap();
    let object = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let output = session
        .owner_token()
        .unwrap()
        .node_factory()
        .strings(&["binary/output.bin"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&session, &[object, output]);
    session.with_active_in(|instance| unsafe {
        let arena = &mut (*instance).arena;
        arena.set_budget(crate::sexp::memory::ArenaBudget::new(
            arena.total_bytes_allocated() + 1000,
            0,
        ));
    });
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        filename_owned::write(args.clone())
    }))
    .unwrap_err();
    assert!(
        payload
            .downcast_ref::<RError>()
            .unwrap()
            .message
            .contains("memory limit")
    );
    assert_eq!(calls.get(), 0);
    assert!(session.get_browser_file("binary/output.bin").is_none());
    session.with_active_in(|instance| unsafe {
        (*instance)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::unlimited());
    });
    assert!(filename_owned::write(args).unwrap().is_nil());
    assert_eq!(calls.get(), 128);
    assert_eq!(
        session.get_browser_file("binary/output.bin").unwrap().len(),
        1024
    );
}

#[test]
fn owned_binary_filename_zero_shape_and_workspace_overflow_are_checked() {
    let session = session();
    let factory = session.owner_token().unwrap().node_factory();
    let empty = factory.strings(&[]).unwrap().into_owned().unwrap();
    assert!(
        filename_owned::read(read_arguments(&session, empty, None))
            .unwrap_err()
            .contains("description")
    );
    let two = factory
        .strings(&["binary/input.bin", "binary/input.bin"])
        .unwrap()
        .into_owned()
        .unwrap();
    assert!(
        filename_owned::read(read_arguments(&session, two, None))
            .unwrap_err()
            .contains("description")
    );
    let parameters = FilenameReadParameters {
        kind: BinaryKind::Complex,
        count: usize::MAX,
        size: 1,
        signed: true,
        order: ByteOrder::Little,
    };
    assert!(parameters.workspace(usize::MAX).is_err());
    let parameters = FilenameReadParameters {
        kind: BinaryKind::Raw,
        count: 0,
        size: 1,
        signed: true,
        order: ByteOrder::Little,
    };
    assert_eq!(parameters.workspace(usize::MAX).unwrap(), 0);
    let zero = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap()
        .into_owned()
        .unwrap();
    assert_eq!(zero.try_integer_elt(0).unwrap(), 0);
    let result =
        filename_owned::read(read_arguments(&session, filename(&session), Some(zero))).unwrap();
    assert_eq!(result.typeof_(), SEXPTYPE::RAWSXP);
    assert_eq!(result.len(), 0);
}

#[test]
fn owned_binary_filename_character_result_survives_full_gc_during_construction() {
    let mut session = session();
    session
        .put_browser_file("binary/input.bin", b"alpha\0beta\0")
        .unwrap();
    let factory = session.owner_token().unwrap().node_factory();
    let what = factory
        .strings(&["character"])
        .unwrap()
        .into_owned()
        .unwrap();
    let count = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut count = crate::sexp::SexpMut::try_from_checked(count).unwrap();
    count.try_set_integer_elt(0, 2).unwrap();
    let args = arguments(
        &session,
        &[
            filename(&session),
            what,
            count.freeze().into_owned().unwrap(),
        ],
    );
    let observed = Rc::new(Cell::new(0));
    let callback_observed = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        callback_observed.set(callback_observed.get() + 1);
    }));
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
    });
    let result = filename_owned::read(args).unwrap();
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 0;
        (*instance).memory_state.gc_force_wait = 0;
    });
    assert!(observed.get() >= 3);
    session.with_active(crate::sexp::gengc::full_gc);
    assert_eq!(
        result.try_string_value_elt(0).unwrap().as_deref(),
        Some("alpha")
    );
    assert_eq!(
        result.try_string_value_elt(1).unwrap().as_deref(),
        Some("beta")
    );
}

#[test]
fn owned_binary_filename_character_encoder_admits_realloc_and_string_peak() {
    let mut session = session();
    let factory = session.owner_token().unwrap().node_factory();
    // Appending the second string grows the byte buffer while its old buffer
    // and copied string are both still alive. 3N covers final capacity + sink
    // but can miss this earlier peak (130 + 260 + 64 > 3 * 131).
    let object = factory
        .strings(&[&"a".repeat(65), &"b".repeat(64)])
        .unwrap()
        .into_owned()
        .unwrap();
    let size = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut size = crate::sexp::SexpMut::try_from_checked(size).unwrap();
    size.try_set_integer_elt(0, crate::sexp::ffi::NA_INTEGER)
        .unwrap();
    let size = size.freeze().into_owned().unwrap();
    let owner = object.runtime_owner.as_ref().unwrap().clone();
    session.with_active_in(|instance| unsafe {
        let arena = &mut (*instance).arena;
        arena.set_budget(crate::sexp::memory::ArenaBudget::new(
            arena.total_bytes_allocated() + 420,
            0,
        ));
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(&owner, |access| {
            filename_encode(
                access,
                [
                    &object,
                    &size,
                    &access.domain().missing(),
                    &access.domain().logical(false),
                ],
            )
        })
    }));
    let payload = match outcome {
        Err(payload) => payload,
        Ok(_) => panic!("character reallocation peak was not admitted"),
    };
    assert!(
        payload
            .downcast_ref::<RError>()
            .unwrap()
            .message
            .contains("memory limit")
    );
    assert!(session.get_browser_file("binary/output.bin").is_none());
    session.with_active_in(|instance| unsafe {
        (*instance)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::unlimited());
    });
    let (bytes, reservations) = crate::sexp::owner::with_runtime(&owner, |access| {
        filename_encode(
            access,
            [
                &object,
                &size,
                &access.domain().missing(),
                &access.domain().logical(false),
            ],
        )
    })
    .unwrap()
    .unwrap();
    assert!(!reservations.is_empty());
    assert_eq!(bytes.len(), 131);
    assert_eq!(&bytes[..65], &[b'a'; 65]);
    assert_eq!(bytes[65], 0);
    assert_eq!(&bytes[66..130], &[b'b'; 64]);
    assert_eq!(bytes[130], 0);
}

#[test]
fn owned_binary_filename_host_reader_admits_reallocation_and_stack_workspace() {
    let session = RSession::new_for_gc_tests();
    struct TemporaryFile(std::path::PathBuf);
    impl Drop for TemporaryFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let path = std::env::temp_dir().join(format!(
        "rport-binary-host-{}-{}.bin",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let _cleanup = TemporaryFile(path.clone());
    std::io::Write::write_all(&mut file, &vec![37_u8; 32769]).unwrap();
    drop(file);
    let factory = session.owner_token().unwrap().node_factory();
    let con = factory
        .strings(&[path.to_str().unwrap()])
        .unwrap()
        .into_owned()
        .unwrap();
    let count = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut count = crate::sexp::SexpMut::try_from_checked(count).unwrap();
    count.try_set_integer_elt(0, 32769).unwrap();
    let args = read_arguments(&session, con, Some(count.freeze().into_owned().unwrap()));
    session.with_active_in(|instance| unsafe {
        let arena = &mut (*instance).arena;
        arena.set_budget(crate::sexp::memory::ArenaBudget::new(
            arena.total_bytes_allocated() + 70000,
            0,
        ));
    });
    // The final byte reallocates the complete 32 KiB buffer while its old
    // storage and 8 KiB stack chunk remain live. The old final-length-only
    // reservation let this pass, even though that peak exceeds 70000 bytes.
    match filename_owned::read(args.clone()) {
        Err(message) => assert!(message.contains("binary file buffer"), "{message}"),
        Ok(_) => panic!("host reallocation and stack peak were not admitted"),
    }
    session.with_active_in(|instance| unsafe {
        (*instance)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::unlimited());
    });
    let result = filename_owned::read(args).unwrap();
    assert_eq!(result.len(), 32769);
    assert_eq!(result.try_raw_elt(32768).unwrap(), 37);
}

#[test]
fn owned_binary_filename_saved_character_survives_admission_slot_detachment_and_gc() {
    let session = session();
    let factory = session.owner_token().unwrap().node_factory();
    let object = factory
        .strings(&["original"])
        .unwrap()
        .into_owned()
        .unwrap();
    let replacement = factory
        .character("replacement")
        .unwrap()
        .into_owned()
        .unwrap();
    let size = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut size = crate::sexp::SexpMut::try_from_checked(size).unwrap();
    size.try_set_integer_elt(0, crate::sexp::ffi::NA_INTEGER)
        .unwrap();
    let size = size.freeze().into_owned().unwrap();
    let observed = Cell::new(0);
    let owner = object.runtime_owner.as_ref().unwrap().clone();
    let bytes = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_native(|_| unsafe {
            Ok(encode_binary_object_admitted(
                object.as_raw(),
                size.as_raw(),
                ByteOrder::Little,
                |_| {
                    observed.set(observed.get() + 1);
                    let mut object =
                        crate::sexp::SexpMut::try_from_checked(object.clone()).unwrap();
                    object.try_set_string_elt(0, replacement.clone()).unwrap();
                    drop(object);
                    crate::sexp::gengc::full_gc();
                },
            ))
        })
    })
    .unwrap()
    .unwrap();
    assert_eq!(observed.get(), 1);
    assert_eq!(bytes, b"original\0");
    assert_eq!(
        object.try_string_value_elt(0).unwrap().as_deref(),
        Some("replacement")
    );
}

fn malformed_binary_arguments(malformed: &str) {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let class = session
        .register_altrep_class(
            "binary.admission",
            FilenameProvider(Rc::new(move || {
                observed.set(observed.get() + 1);
            })),
        )
        .unwrap();
    let con = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let nil = factory.nil();
    let what = factory.strings(&["raw"]).unwrap().into_owned().unwrap();
    let con_tag = session.with_active(|| unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(c"con".as_ptr()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let bad_tag = if malformed == "tag" {
        what.clone()
    } else {
        nil.clone()
    };
    let tail = factory
        .pairlist_cell(&what, &nil, &bad_tag)
        .unwrap()
        .into_owned()
        .unwrap();
    let args = factory
        .pairlist_cell(&con, &tail, &con_tag)
        .unwrap()
        .into_owned()
        .unwrap();
    let foreign = RSession::new_for_gc_tests();
    let foreign_value = foreign
        .owner_token()
        .unwrap()
        .node_factory()
        .strings(&["foreign"])
        .unwrap()
        .into_owned()
        .unwrap();
    let node = tail.allocation().unwrap();
    let heap = node.heap_identity();
    let mut header = heap.node_snapshot(node).unwrap();
    let crate::sexp::ffi::NodeBody::List(ref mut list) = header.data else {
        unreachable!()
    };
    match malformed {
        "cycle" => list.cdrval = args.allocation().unwrap().link().unwrap(),
        "tail" => list.cdrval = what.allocation().unwrap().link().unwrap(),
        "foreign" => list.carval = foreign_value.allocation().unwrap().link().unwrap(),
        "tag" => (),
        _ => unreachable!(),
    }
    heap.replace_node(node, header).unwrap();
    let result = session.with_active(|| filename_owned::read(args));
    assert_eq!(
        calls.get(),
        0,
        "{malformed} reached filename provider: {result:?}"
    );
    let error = result.unwrap_err();
    assert!(
        !error.contains("activate"),
        "fixture missed admission: {error}"
    );
    if malformed == "cycle" {
        assert!(error.contains("cyclic binary argument list"), "{error}");
    }
}

#[test]
fn binary_admission_cycle_precedes_filename_provider() {
    malformed_binary_arguments("cycle");
}
#[test]
fn binary_admission_improper_tail_precedes_filename_provider() {
    malformed_binary_arguments("tail");
}
#[test]
fn binary_admission_improper_tag_precedes_filename_provider() {
    malformed_binary_arguments("tag");
}
#[test]
fn binary_admission_foreign_operand_precedes_filename_provider() {
    malformed_binary_arguments("foreign");
}

#[test]
fn binary_admission_named_con_keeps_first_unnamed_what_and_count() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let con = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 4)))
        .unwrap()
        .into_owned()
        .unwrap();
    for (index, byte) in [9, 8, 7, 6].into_iter().enumerate() {
        unsafe {
            SET_RAW_ELT(con.as_raw(), index as c_int, byte);
        }
    }
    let what = factory.strings(&["raw"]).unwrap().into_owned().unwrap();
    let count = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap()
        .into_owned()
        .unwrap();
    unsafe { SET_INTEGER_ELT(count.as_raw(), 0, 2) };
    let con_tag = session.with_active(|| unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(c"con".as_ptr()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let rest = arguments(&session, &[what, count]);
    let args = factory
        .pairlist_cell(&con, &rest, &con_tag)
        .unwrap()
        .into_owned()
        .unwrap();
    let output = filename_owned::read(args).unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(output.try_raw_elt(0).unwrap(), 9);
    assert_eq!(output.try_raw_elt(1).unwrap(), 8);
}
