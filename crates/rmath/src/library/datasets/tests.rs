use super::*;
use crate::sexp::{RSession, SEXPTYPE, SexpMut};
use std::{cell::Cell, rc::Rc};

fn integers(session: &RSession, values: &[i32]) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    let value = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as _)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (index, item) in values.iter().copied().enumerate() {
        value.try_set_integer_elt(index as _, item).unwrap();
    }
    value.freeze().into_owned().unwrap()
}

fn arguments(session: &RSession, name: &str) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    let &(_, offset, size) = inventory::OBJECTS
        .iter()
        .find(|(object, _, _)| *object == name)
        .unwrap();
    let key = integers(session, &[offset, size]);
    let file = factory.strings(&[DATABASE]).unwrap().into_owned().unwrap();
    let compressed = integers(session, &[3]);
    let nil = factory.nil();
    let hook = factory
        .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP)))
        .unwrap();
    let tail = factory.pairlist_cell(&hook, &nil, &nil).unwrap();
    let tail = factory.pairlist_cell(&compressed, &tail, &nil).unwrap();
    let tail = factory.pairlist_cell(&file, &tail, &nil).unwrap();
    factory
        .pairlist_cell(&key, &tail, &nil)
        .unwrap()
        .into_owned()
        .unwrap()
}

fn fetch_arguments(session: &RSession, arguments: &Sexp<'static>) -> Sexp<'static> {
    let factory = session.owner_token().unwrap().node_factory();
    let nil = factory.nil();
    // Genuine production primitive adapter; no replacement lazy-fetch wrapper.
    let raw = unsafe {
        crate::mainutils::serialize::do_lazyLoadDBfetch(
            nil.as_raw(),
            nil.as_raw(),
            arguments.as_raw(),
            nil.as_raw(),
        )
    };
    factory.wrap(raw).unwrap().into_owned().unwrap()
}

fn first_collection(session: &RSession) {
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
    });
}

fn stop_forcing(session: &RSession) {
    session.with_active_in(|instance| unsafe {
        (*instance).memory_state.gc_force_gap = 0;
        (*instance).memory_state.gc_force_wait = 0;
    });
}

#[test]
fn owned_portable_dataset_literal_inventory_matches_authenticated_assets() {
    let inventory_json: serde_json::Value =
        serde_json::from_str(include_str!("assets/inventory.json")).unwrap();
    let objects = inventory_json["objects"].as_array().unwrap();
    assert_eq!(objects.len(), inventory::OBJECTS.len());
    let mut end = 0;
    for (json, &(name, start, count)) in objects.iter().zip(inventory::OBJECTS) {
        assert_eq!(json["name"].as_str().unwrap(), name);
        assert_eq!(
            json["offset"].as_str().unwrap().parse::<i32>().unwrap(),
            start
        );
        assert_eq!(
            json["bytes"].as_str().unwrap().parse::<i32>().unwrap(),
            count
        );
        assert_eq!(start, end);
        assert!(count >= 5);
        end += count;
    }
    assert_eq!(end as usize, include_bytes!("assets/Rdata.rdb").len());
    let index_json = inventory_json["index"].as_array().unwrap();
    assert_eq!(index_json.len(), inventory::INDEX.len());
    for (json, &(item, title)) in index_json.iter().zip(inventory::INDEX) {
        assert_eq!(json["item"].as_str().unwrap(), item);
        assert_eq!(json["title"].as_str().unwrap(), title);
    }
    let topics_json = inventory_json["topics"].as_object().unwrap();
    assert_eq!(topics_json.len(), inventory::TOPICS.len());
    for &(topic, names) in inventory::TOPICS {
        let original: Vec<_> = topics_json[topic]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        assert_eq!(original, names);
    }
}

#[test]
fn owned_portable_dataset_saved_promise_graph_survives_namespace_detachment_and_full_gc() {
    let session = RSession::new_for_gc_tests();
    let namespace = namespace().unwrap();
    let owner = namespace.runtime_owner.as_ref().unwrap().clone();
    let promise = crate::sexp::owner::with_runtime(&owner, |access| {
        let info = lookup(access, &namespace, ".__NAMESPACE__.").unwrap();
        let lazy = lookup(access, &info, "lazydata").unwrap();
        lookup(access, &lazy, "mtcars").unwrap()
    })
    .unwrap();
    assert_eq!(promise.typeof_(), SEXPTYPE::PROMSXP);
    let expression_identity = promise.try_prcode().unwrap().allocation().unwrap().clone();
    let captured_identity = promise.try_prenv().unwrap().allocation().unwrap().clone();
    // Metadata identities retain no Sexp roots. Remove the whole namespace root;
    // the single saved promise is the only remaining owner of its source graph.
    session.with_active_in(|instance| unsafe { (*instance).package_namespace_cache.clear() });
    drop(namespace);
    let observed = Rc::new(Cell::new(false));
    let callback_observed = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| callback_observed.set(true)));
    session.with_active(crate::sexp::gengc::full_gc);
    assert!(observed.get());
    assert!(expression_identity.is_live());
    assert!(captured_identity.is_live());
    let expression = promise.try_prcode().unwrap();
    assert!(
        expression
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_char_eq(b"lazyLoadDBfetch")
            .unwrap()
    );
    let key = expression.try_cdr().unwrap().try_car().unwrap();
    assert_eq!(key.typeof_(), SEXPTYPE::INTSXP);
    assert_eq!(key.try_integer_elt(0).unwrap(), 59685);
    assert_eq!(key.try_integer_elt(1).unwrap(), 1102);
    let captured = promise.try_prenv().unwrap();
    let original_file = crate::sexp::owner::with_runtime(&owner, |access| {
        lookup(access, &captured.into_owned().unwrap(), "datafile")
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        original_file.try_string_value_elt(0).unwrap().as_deref(),
        Some(DATABASE)
    );
}

#[test]
fn owned_portable_dataset_private_lookup_reads_real_namespace_lazydata() {
    let session = RSession::new_for_gc_tests();
    let namespace = namespace().unwrap();
    let names = session.with_active(|| unsafe {
        crate::mainutils::essentials::lazy_data_names_binding(namespace.as_raw())
    });
    let expected: Vec<_> = inventory::OBJECTS
        .iter()
        .map(|(name, _, _)| (*name).to_string())
        .collect();
    assert_eq!(names, expected);
    let frame_names = session.with_active(|| unsafe {
        crate::mainutils::essentials::frame_binding_names(namespace.as_raw(), true)
    });
    // The GNU namespace metadata belongs to .__NAMESPACE__., never a hidden
    // extra dataset index inserted into the namespace's ordinary frame.
    assert_eq!(frame_names, [".__NAMESPACE__.", ".__S3MethodsTable__."]);
}

#[test]
fn owned_portable_dataset_fetch_keeps_all_selected_operands_after_argument_detachment_and_full_gc()
{
    let session = RSession::new_for_gc_tests();
    let arguments = arguments(&session, "euro");
    let argument_identity = arguments.allocation().unwrap().clone();
    let selected: Vec<_> = (0..4)
        .map(|i| {
            arguments
                .clone()
                .try_pairlist_arg(i)
                .unwrap()
                .allocation()
                .ok()
                .cloned()
        })
        .collect();
    let observed = Rc::new(Cell::new(false));
    let callback_observed = observed.clone();
    let original_owner = arguments.runtime_owner.as_ref().unwrap().clone();
    let domain =
        crate::sexp::owner::with_runtime(&original_owner, |access| access.domain()).unwrap();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if callback_observed.replace(true) {
            return;
        }
        let owner = unsafe { OwnerToken::current() }.unwrap();
        unsafe {
            (*owner.as_ptr()).memory_state.gc_force_gap = 0;
            (*owner.as_ptr()).memory_state.gc_force_wait = 0;
        }
        let args = domain
            .wrap(
                argument_identity
                    .heap_identity()
                    .projection_of_link(argument_identity.link().unwrap())
                    .unwrap(),
            )
            .unwrap();
        unsafe {
            crate::sexp::accessors::SETCAR(args.as_raw(), domain.nil().as_raw());
            crate::sexp::accessors::SETCDR(args.as_raw(), domain.nil().as_raw());
        }
        crate::sexp::gengc::full_gc();
        for identity in selected.iter().flatten() {
            assert!(
                identity.is_live(),
                "selected operands must own independent roots"
            );
            assert!(identity.root_count() > 0);
        }
    }));
    first_collection(&session);
    let result = fetch_arguments(&session, &arguments);
    assert!(observed.get());
    assert!(arguments.try_car().unwrap().is_nil());
    assert!(arguments.try_cdr().unwrap().is_nil());
    assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(result.len(), 11);
    assert_eq!(result.try_real_elt(0).unwrap(), 13.7603);
    assert_eq!(result.try_real_elt(10).unwrap(), 200.482);
}

#[test]
fn owned_portable_dataset_fetch_revocation_cannot_publish_a_result() {
    let session = RSession::new_for_gc_tests();
    let arguments = arguments(&session, "euro");
    let owner = arguments.runtime_owner.as_ref().unwrap().clone();
    let observed = Rc::new(Cell::new(false));
    let callback_observed = observed.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if callback_observed.replace(true) {
            return;
        }
        let pin = owner.pin().unwrap();
        unsafe { crate::sexp::instance::revoke_instance_availability(pin.as_ptr()) };
    }));
    first_collection(&session);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fetch_arguments(&session, &arguments)
    }));
    assert!(observed.get());
    let payload = match result {
        Err(payload) => payload,
        Ok(_) => panic!("revoked virtual fetch published a value"),
    };
    let error = payload.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        crate::sexp::SexpError::RootUnavailable.to_string()
    );
    assert!(!session.is_active());
}

#[test]
fn owned_portable_dataset_decompressed_output_survives_its_allocation_callback() {
    let session = RSession::new_for_gc_tests();
    let arguments = arguments(&session, "euro");
    let collections = Rc::new(Cell::new(0));
    let callback_collections = collections.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        let count = callback_collections.get() + 1;
        callback_collections.set(count);
        if count != 2 {
            return;
        }
        // First allocation is the selected compressed record. The second is
        // its decoded raw output: collect while that producer is still active.
        let owner = unsafe { OwnerToken::current() }.unwrap();
        unsafe {
            (*owner.as_ptr()).memory_state.gc_force_gap = 0;
            (*owner.as_ptr()).memory_state.gc_force_wait = 0;
        }
        crate::sexp::gengc::full_gc();
    }));
    first_collection(&session);
    let result = fetch_arguments(&session, &arguments);
    assert!(collections.get() >= 2);
    assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(result.len(), 11);
    assert_eq!(result.try_real_elt(0).unwrap(), 13.7603);
    assert_eq!(result.try_real_elt(10).unwrap(), 200.482);
}

#[test]
fn owned_portable_dataset_fetch_preserves_live_panic_and_recovers() {
    let session = RSession::new_for_gc_tests();
    let arguments = arguments(&session, "euro");
    crate::sexp::gengc::register_gc_callback(Box::new(|_| std::panic::panic_any(0xDA7A_2142_u32)));
    first_collection(&session);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fetch_arguments(&session, &arguments)
    }));
    let payload = match result {
        Err(payload) => payload,
        Ok(_) => panic!("live callback panic lost"),
    };
    assert_eq!(*payload.downcast::<u32>().unwrap(), 0xDA7A_2142);
    stop_forcing(&session);
    session.with_active_in(|instance| unsafe { (*instance).gc_state.callbacks.clear() });
    let result = fetch_arguments(&session, &arguments);
    assert_eq!(result.len(), 11);
    assert_eq!(result.try_real_elt(10).unwrap(), 200.482);
}

#[test]
fn owned_portable_dataset_virtual_keys_reject_malformed_and_foreign_input_before_publication() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let file = factory.strings(&[DATABASE]).unwrap().into_owned().unwrap();
    for values in [vec![0], vec![-1, 380], vec![1, 379], vec![0, i32::MAX]] {
        assert_eq!(
            read_database(file.clone(), integers(&session, &values)).unwrap_err(),
            "bad offset/length argument"
        );
    }
    let other = RSession::new_for_gc_tests();
    let foreign_key = integers(&other, &[0, 380]);
    let foreign_error = crate::sexp::SexpError::UnownedPointer {
        address: foreign_key.as_raw().addr(),
    }
    .to_string();
    session.with_active(|| {
        let error = read_database(file, foreign_key).unwrap_err();
        assert_eq!(error, foreign_error);
    });
    // Original runtime is still usable and literal original ranges recover.
    session.with_active(|| {
        let result = fetch_arguments(&session, &arguments(&session, "euro"));
        assert_eq!(result.len(), 11);
    });
}
