use crate::sexp::{RSession, SEXPTYPE, Sexp};
use std::path::PathBuf;

struct PackageFixture(PathBuf);
impl PackageFixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let identity = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "rport-serialized-data-{}-{identity}",
            std::process::id()
        ));
        let data = directory.join("serializedfixture/data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            directory.join("serializedfixture/DESCRIPTION"),
            "Package: serializedfixture\nVersion: 1.0\n",
        )
        .unwrap();
        std::fs::write(
            data.join("original.rda"),
            include_bytes!("fixtures/workspace-gzip.rda"),
        )
        .unwrap();
        Self(directory)
    }
}
impl Drop for PackageFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn public_package_data_loads_original_serialized_workspace() {
    let directory = PackageFixture::new();
    let mut session = RSession::new_for_gc_tests();
    session.set_library_paths([&directory.0]);
    let factory = session.owner_token().unwrap().node_factory();
    let environment = factory
        .wrap(unsafe {
            crate::sexp::memory_ext::NewEnvironment(
                factory.nil().as_raw(),
                crate::sexp::globals::R_EmptyEnv(),
                factory.nil().as_raw(),
            )
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let loaded = unsafe {
        super::super::shared::load_package_data_set(
            "original",
            &["serializedfixture".to_owned()],
            environment.as_raw(),
        )
    }
    .expect("GNU serialized data file must load");
    assert!(loaded);
    let symbol = unsafe { crate::sexp::symbol::Rf_install(c"frame".as_ptr()) };
    let value = factory
        .wrap(unsafe { crate::sexp::envir::R_findVarInFrame(environment.as_raw(), symbol) })
        .unwrap();
    assert_eq!(value.typeof_(), SEXPTYPE::VECSXP);
    assert_eq!(value.len(), 3);
    assert_eq!(
        value.try_vector_elt(0).unwrap().try_real_elt(0).unwrap(),
        1.25
    );
}

fn environment(session: &RSession) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    factory
        .wrap(unsafe {
            crate::sexp::memory_ext::NewEnvironment(
                factory.nil().as_raw(),
                crate::sexp::globals::R_EmptyEnv(),
                factory.nil().as_raw(),
            )
        })
        .unwrap()
        .into_owned()
        .unwrap()
}

fn binding(environment: &Sexp<'static>, name: &[u8]) -> Option<Sexp<'static>> {
    let mut frame = environment.try_frame().unwrap();
    while !frame.is_nil() {
        if frame.try_tag_name_eq(name).unwrap() {
            return Some(frame.try_car().unwrap().into_owned().unwrap());
        }
        frame = frame.try_cdr().unwrap();
    }
    None
}

fn attribute(value: &Sexp<'static>, name: &[u8]) -> Sexp<'static> {
    let mut attributes = value.try_attrib().unwrap();
    while !attributes.is_nil() {
        if attributes.try_tag_name_eq(name).unwrap() {
            return attributes.try_car().unwrap().into_owned().unwrap();
        }
        attributes = attributes.try_cdr().unwrap();
    }
    panic!("missing attribute {:?}", name);
}

fn assert_original_graph(environment: &Sexp<'static>) {
    let frame = binding(environment, b"frame").unwrap();
    assert_eq!(frame.typeof_(), SEXPTYPE::VECSXP);
    assert_eq!(frame.len(), 3);
    assert_eq!(
        attribute(&frame, b"class")
            .try_string_value_elt(0)
            .unwrap()
            .as_deref(),
        Some("data.frame")
    );
    assert_eq!(
        attribute(&frame, b"row.names")
            .try_string_value_elt(1)
            .unwrap()
            .as_deref(),
        Some("second")
    );
    let values = frame.try_vector_elt(0).unwrap();
    assert_eq!(values.try_real_elt(0).unwrap(), 1.25);
    assert!(crate::sexp::ffi::R_IsNA(values.try_real_elt(1).unwrap()));
    let factor = frame.try_vector_elt(1).unwrap().into_owned().unwrap();
    assert_eq!(factor.try_integer_elt(0).unwrap(), 2);
    assert_eq!(
        attribute(&factor, b"levels")
            .try_string_value_elt(0)
            .unwrap()
            .as_deref(),
        Some("a")
    );
    let date = frame.try_vector_elt(2).unwrap().into_owned().unwrap();
    assert_eq!(
        attribute(&date, b"class")
            .try_string_value_elt(0)
            .unwrap()
            .as_deref(),
        Some("Date")
    );
    assert_eq!(date.try_real_elt(0).unwrap(), 10957.);
    let shared_a = binding(environment, b"shared_a").unwrap();
    let shared_b = binding(environment, b"shared_b").unwrap();
    assert_eq!(shared_a, shared_b);
    assert_eq!(binding(&shared_a, b"self").unwrap(), shared_a);
    assert_eq!(
        binding(&shared_a, b"value")
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        37
    );
    assert!(binding(environment, b"null_value").unwrap().is_nil());
    let named = binding(environment, b"named_na").unwrap();
    assert_eq!(
        named.try_integer_elt(0).unwrap(),
        crate::sexp::ffi::NA_INTEGER
    );
    assert_eq!(
        attribute(&named, b"names")
            .try_string_value_elt(1)
            .unwrap()
            .as_deref(),
        Some("second")
    );
}

#[test]
fn owned_workspace_preserves_gnu_formats_attributes_and_environment_identity() {
    for bytes in [
        include_bytes!("fixtures/workspace-v2.rda").as_slice(),
        include_bytes!("fixtures/workspace-v3.rda").as_slice(),
        include_bytes!("fixtures/workspace-gzip.rda").as_slice(),
        include_bytes!("fixtures/workspace-bzip2.rda").as_slice(),
        include_bytes!("fixtures/workspace-xz.rda").as_slice(),
        include_bytes!("fixtures/workspace-ascii.rda").as_slice(),
    ] {
        let session = RSession::new_for_gc_tests();
        let target = environment(&session);
        let names = super::load_bytes(bytes, target.clone()).unwrap();
        assert_eq!(
            names,
            ["frame", "shared_a", "shared_b", "null_value", "named_na"]
        );
        // The decoder and its graph have left scope; only published bindings own data.
        crate::sexp::gengc::full_gc();
        assert_original_graph(&target);
    }
}

fn evaluate(session: &RSession, source: &str) -> Sexp<'static> {
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let expression = owner
        .with_arena(|arena| crate::eval::parser::parse(source, arena, factory.domain()))
        .unwrap()
        .unwrap();
    factory
        .wrap(unsafe {
            crate::eval::eval::Rf_eval(expression.as_raw(), session.global_env().unwrap().as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap()
}

#[test]
fn owned_workspace_pending_values_survive_collecting_binding_callback() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "{writes<-0L; makeActiveBinding('frame', function(value){writes<<-writes+1L;gc();invisible(NULL)}, globalenv())}",
    ));
    let target = session.global_env().unwrap().into_owned().unwrap();
    let observed = std::rc::Rc::new(std::cell::Cell::new(0));
    let notifications = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        notifications.set(notifications.get() + 1)
    }));
    let names =
        super::load_bytes(include_bytes!("fixtures/workspace-v3.rda"), target.clone()).unwrap();
    assert_eq!(names.len(), 5);
    assert!(
        observed.get() > 0,
        "the original first setter must force collection"
    );
    assert_eq!(
        binding(&target, b"writes")
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        1
    );
    let a = binding(&target, b"shared_a").unwrap();
    assert_eq!(a, binding(&target, b"shared_b").unwrap());
    assert_eq!(a, binding(&a, b"self").unwrap());
    assert!(binding(&target, b"null_value").unwrap().is_nil());
    assert_eq!(
        binding(&target, b"named_na")
            .unwrap()
            .try_integer_elt(1)
            .unwrap(),
        9
    );
}

#[test]
fn owned_workspace_revocation_refuses_remaining_binding_publication() {
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "makeActiveBinding('frame', function(value){gc();invisible(NULL)}, globalenv())",
    ));
    let target = session.global_env().unwrap().into_owned().unwrap();
    let owner = target.runtime_owner.as_ref().unwrap().clone();
    let revoked = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = revoked.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !observed.replace(true) {
            let pin = owner.pin().unwrap();
            unsafe {
                crate::sexp::instance::revoke_instance_availability(pin.as_ptr());
            }
        }
    }));
    let error =
        super::load_bytes(include_bytes!("fixtures/workspace-v3.rda"), target.clone()).unwrap_err();
    assert!(revoked.get());
    assert_eq!(error, crate::sexp::SexpError::RootUnavailable.to_string());
    assert!(binding(&target, b"shared_a").is_none());
    assert!(binding(&target, b"named_na").is_none());
}

#[test]
fn owned_workspace_rejects_bad_streams_and_loans_before_publication() {
    let session = RSession::new_for_gc_tests();
    let target = environment(&session);
    for bytes in [
        b"".as_slice(),
        b"RDX3\nA\n".as_slice(),
        b"RDX9\nX\n".as_slice(),
        &include_bytes!("fixtures/workspace-gzip.rda")[..15],
    ] {
        assert!(super::load_bytes(bytes, target.clone()).is_err());
        assert!(target.try_frame().unwrap().is_nil());
    }
    let error = session
        .owner_token()
        .unwrap()
        .with_arena(|_| {
            super::load_bytes(include_bytes!("fixtures/workspace-v3.rda"), target.clone())
                .unwrap_err()
        })
        .unwrap();
    assert_eq!(error, crate::sexp::SexpError::OwnerNotActive.to_string());
    assert!(target.try_frame().unwrap().is_nil());
}

#[test]
fn owned_workspace_graph_admission_bounds_cycles_and_invalid_tags() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil().into_owned().unwrap();
    let cell = factory
        .pairlist_cell(&nil, &nil, &nil)
        .unwrap()
        .into_owned()
        .unwrap();
    assert!(
        super::owned::bindings(cell.clone())
            .unwrap_err()
            .contains("binding name")
    );
    let symbol = factory
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"cycle".as_ptr()) })
        .unwrap();
    let cell = factory
        .pairlist_cell(&nil, &nil, &symbol)
        .unwrap()
        .into_owned()
        .unwrap();
    unsafe {
        crate::sexp::accessors::SETCDR(cell.as_raw(), cell.as_raw());
    }
    assert!(super::owned::bindings(cell).unwrap_err().contains("cyclic"));
}

#[test]
fn owned_workspace_budget_error_and_foreign_authority_leave_target_unchanged() {
    let mut session = RSession::new_for_gc_tests();
    let target = environment(&session);
    let bytes = session
        .owner_token()
        .unwrap()
        .with_arena(|arena| arena.total_bytes_allocated())
        .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(bytes + 64, 0));
    let error = super::load_bytes(
        include_bytes!("fixtures/workspace-gzip.rda"),
        target.clone(),
    )
    .unwrap_err();
    assert!(error.contains("allocation failed"), "{error}");
    assert!(target.try_frame().unwrap().is_nil());
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::unlimited());
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let error = super::load_bytes(include_bytes!("fixtures/workspace-v3.rda"), target.clone())
            .unwrap_err();
        assert_eq!(error, crate::sexp::SexpError::OwnerNotActive.to_string());
        assert!(target.try_frame().unwrap().is_nil());
    });
}

#[test]
fn public_data_primitive_loads_serialized_topic_into_explicit_environment() {
    let directory = PackageFixture::new();
    let mut session = RSession::new_for_gc_tests();
    session.set_library_paths([&directory.0]);
    let target = environment(&session);
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil().into_owned().unwrap();
    let topic = factory.strings(&["original"]).unwrap();
    let package = factory.strings(&["serializedfixture"]).unwrap();
    let args = factory.pairlist_cell(&target, &nil, &nil).unwrap();
    let args = factory.pairlist_cell(&package, &args, &nil).unwrap();
    let args = factory.pairlist_cell(&topic, &args, &nil).unwrap();
    let loaded = factory
        .wrap(unsafe {
            super::super::runtime::do_data(nil.as_raw(), nil.as_raw(), args.as_raw(), nil.as_raw())
        })
        .unwrap();
    assert_eq!(
        loaded.try_string_value_elt(0).unwrap().as_deref(),
        Some("original")
    );
    assert_original_graph(&target);
}

#[test]
fn public_package_data_indexes_and_loads_rdata_and_rejects_corrupt_files() {
    let directory = PackageFixture::new();
    let data = directory.0.join("serializedfixture/data");
    std::fs::write(
        data.join("alternate.RData"),
        include_bytes!("fixtures/workspace-v2.rda"),
    )
    .unwrap();
    let mut session = RSession::new_for_gc_tests();
    session.set_library_paths([&directory.0]);
    let packages = ["serializedfixture".to_owned()];
    assert_eq!(
        super::super::shared::list_package_data_sets(&packages),
        ["alternate", "original"]
    );
    let target = environment(&session);
    assert!(
        unsafe {
            super::super::shared::load_package_data_set("alternate", &packages, target.as_raw())
        }
        .unwrap()
    );
    assert_original_graph(&target);
    std::fs::write(data.join("broken.rda"), b"unsupported serialized data").unwrap();
    let empty = environment(&session);
    let error =
        unsafe { super::super::shared::load_package_data_set("broken", &packages, empty.as_raw()) }
            .unwrap_err();
    assert!(error.contains("bad restore file magic number"), "{error}");
    assert!(empty.try_frame().unwrap().is_nil());
}

#[test]
fn owned_workspace_live_callback_panic_preserves_payload_and_stops_publication() {
    #[derive(Debug)]
    struct Payload(u64);
    let session = RSession::new_for_gc_tests();
    drop(evaluate(
        &session,
        "makeActiveBinding('frame', function(value){gc();invisible(NULL)}, globalenv())",
    ));
    let target = session.global_env().unwrap().into_owned().unwrap();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| std::panic::panic_any(Payload(91))));
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::load_bytes(include_bytes!("fixtures/workspace-v3.rda"), target.clone())
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<Payload>().unwrap().0, 91);
    assert!(binding(&target, b"shared_a").is_none());
    assert!(binding(&target, b"named_na").is_none());
}
