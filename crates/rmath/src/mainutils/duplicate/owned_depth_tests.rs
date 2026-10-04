//! Deep valid copies must not depend on the native Rust thread stack size.

use super::*;
use crate::sexp::{object::SexpMut, session::RSession};
use std::{cell::Cell, rc::Rc};

const MIXED_DEPTH: usize = 32_768;
const WORKER_ENV: &str = "RPORT_OWNED_DUPLICATION_DEPTH_WORKER";
const TEST_NAME: &str = "mainutils::duplicate::owned_depth_tests::owning_depth_valid_mixed_graph_uses_bounded_native_stack";

fn copy_and_check_mixed_graph(depth: usize) {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let mut source = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(107) })
        .unwrap();
    for level in 0..depth {
        source = match level % 3 {
            0 => {
                let value = factory
                    .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
                    .unwrap();
                SexpMut::try_from_checked(value.clone())
                    .unwrap()
                    .try_set_vector_elt(0, source)
                    .unwrap();
                value
            }
            1 => factory
                .pairlist_cell(&source, &factory.nil(), &factory.nil())
                .unwrap(),
            _ => {
                let value = factory
                    .wrap(unsafe { Rf_allocVector3(SEXPTYPE::RAWSXP, 1) })
                    .unwrap();
                let attribute = factory
                    .pairlist_cell(&source, &factory.nil(), &factory.nil())
                    .unwrap();
                unsafe { SET_ATTRIB(value.as_raw(), attribute.as_raw()) };
                value
            }
        };
    }
    println!("constructed valid mixed graph: {depth} layers");
    let protections = crate::sexp::protect::R_ProtectCount();
    let mut copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protections);
    for level in (0..depth).rev() {
        assert_ne!(source.as_raw(), copy.as_raw(), "copy layer {level}");
        assert_eq!(source.typeof_(), copy.typeof_());
        (source, copy) = match level % 3 {
            0 => (
                source.try_vector_elt(0).unwrap(),
                copy.try_vector_elt(0).unwrap(),
            ),
            1 => (source.try_car().unwrap(), copy.try_car().unwrap()),
            _ => {
                assert_eq!(copy.try_raw_elt(0).unwrap(), 0);
                let source_attribute = source.try_attrib().unwrap();
                let copied_attribute = copy.try_attrib().unwrap();
                assert_ne!(source_attribute.as_raw(), copied_attribute.as_raw());
                (
                    source_attribute.try_car().unwrap(),
                    copied_attribute.try_car().unwrap(),
                )
            }
        };
    }
    assert_ne!(source.as_raw(), copy.as_raw());
    assert_eq!(copy.try_integer_elt(0).unwrap(), 107);
    println!("validated distinct mixed copy: {depth} layers");
}

/// A subprocess keeps a real native stack overflow from aborting unrelated
/// tests. The thread owns its session; no runtime owner crosses thread bounds.
#[test]
#[cfg(not(miri))]
fn owning_depth_valid_mixed_graph_uses_bounded_native_stack() {
    if std::env::var_os(WORKER_ENV).as_deref() == Some(std::ffi::OsStr::new("1")) {
        std::thread::Builder::new()
            .name("owned-duplication-depth".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| copy_and_check_mixed_graph(MIXED_DEPTH))
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([TEST_NAME, "--exact", "--nocapture", "--test-threads=1"])
        .env(WORKER_ENV, "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > std::time::Duration::from_mins(1) {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "deep duplication exceeded owned 60-second subprocess bound\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "valid deep graph failed in bounded-stack subprocess: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!(
        "validated distinct mixed copy: {MIXED_DEPTH} layers"
    )));
}

#[test]
fn owning_depth_mixed_continuations_preserve_distinct_copies() {
    copy_and_check_mixed_graph(48);
}

fn nested_vectors<'source>(
    session: &'source RSession,
    mut source: Sexp<'source>,
    depth: usize,
) -> Sexp<'source> {
    let factory = session.owner_token().unwrap().node_factory();
    for _ in 0..depth {
        let parent = factory
            .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
            .unwrap();
        SexpMut::try_from_checked(parent.clone())
            .unwrap()
            .try_set_vector_elt(0, source)
            .unwrap();
        source = parent;
    }
    source
}

#[test]
fn owning_depth_repeated_paths_copy_independently_and_cycles_release_frames() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let scalar = factory
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(113) })
        .unwrap();
    let branch = nested_vectors(&session, scalar, 12);
    let source = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 2) })
        .unwrap();
    let mut mutation = SexpMut::try_from_checked(source.clone()).unwrap();
    mutation.try_set_vector_elt(0, branch.clone()).unwrap();
    mutation.try_set_vector_elt(1, branch).unwrap();
    drop(mutation);
    let state = DuplicationState::default();
    let owner = source.runtime_owner.as_ref().unwrap();
    let copy = crate::sexp::owner::with_runtime(owner, |access| {
        duplicate_iterative(source.clone(), 1, &state, access)
    })
    .unwrap()
    .unwrap();
    assert!(state.active.borrow().is_empty());
    let mut first = copy.try_vector_elt(0).unwrap();
    let mut second = copy.try_vector_elt(1).unwrap();
    for _ in 0..12 {
        assert_ne!(first, second, "each shared occurrence needs its own copy");
        first = first.try_vector_elt(0).unwrap();
        second = second.try_vector_elt(0).unwrap();
    }
    assert_ne!(first, second);
    assert_eq!(first.try_integer_elt(0).unwrap(), 113);
    assert_eq!(second.try_integer_elt(0).unwrap(), 113);

    // A vector -> CAR -> attribute -> CAR -> vector path is cyclic while
    // every individual node and edge has a valid checked shape.
    let cycle = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
        .unwrap();
    let bytes = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::RAWSXP, 1) })
        .unwrap();
    let attributes = factory
        .pairlist_cell(&cycle, &factory.nil(), &factory.nil())
        .unwrap();
    unsafe { SET_ATTRIB(bytes.as_raw(), attributes.as_raw()) };
    let child = factory
        .pairlist_cell(&bytes, &factory.nil(), &factory.nil())
        .unwrap();
    SexpMut::try_from_checked(cycle.clone())
        .unwrap()
        .try_set_vector_elt(0, child)
        .unwrap();
    let protections = crate::sexp::protect::R_ProtectCount();
    let error = crate::sexp::owner::with_runtime(owner, |access| {
        duplicate_iterative(cycle.clone(), 1, &state, access)
    })
    .unwrap()
    .expect_err("mixed deep cycle must be a finite typed error");
    assert!(matches!(error, SexpError::EvaluationFailed { ref message }
        if message == "cyclic object graph cannot be duplicated"));
    assert!(state.active.borrow().is_empty());
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protections);
    let shallow = factory
        .wrap(unsafe { shallow_duplicate(cycle.as_raw()) })
        .unwrap();
    assert_eq!(
        shallow.try_vector_elt(0).unwrap(),
        cycle.try_vector_elt(0).unwrap()
    );
    let recovery = crate::sexp::owner::with_runtime(owner, |access| {
        duplicate_iterative(source.clone(), 1, &state, access)
    })
    .unwrap()
    .unwrap();
    assert_eq!(recovery.len(), 2);
    assert!(state.active.borrow().is_empty());
}

struct CollectAtLeaf {
    reached: Rc<Cell<bool>>,
}

impl crate::sexp::altrep::AltrepClass for CollectAtLeaf {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }

    fn length(&self, _: &crate::sexp::altrep::AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }

    fn element<'source>(
        &self,
        context: &crate::sexp::altrep::AltrepContext<'source>,
        _: i64,
    ) -> SexpResult<crate::sexp::altrep::AltrepElement<'source>> {
        self.reached.set(true);
        context.gc()?;
        Ok(crate::sexp::altrep::AltrepElement::Integer(127))
    }
}

#[test]
fn owning_depth_suspended_frames_keep_detached_nodes_through_collecting_provider() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let reached = Rc::new(Cell::new(false));
    let class = session
        .register_altrep_class(
            "deep-copy-leaf",
            CollectAtLeaf {
                reached: reached.clone(),
            },
        )
        .unwrap();
    let leaf = crate::sexp::altrep::AltrepBuilder::new(class)
        .build()
        .unwrap();
    let source = nested_vectors(&session, leaf, 12);
    let mut current = source.clone();
    let mut nodes = Vec::new();
    for _ in 0..12 {
        nodes.push(current.allocation().unwrap().clone());
        current = current.try_vector_elt(0).unwrap();
    }
    let leaf_node = current.allocation().unwrap().clone();
    drop(current);
    let callback_nodes = nodes.clone();
    let callback_leaf = leaf_node.clone();
    let callback_reached = reached.clone();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    let heap = nodes[0].heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !callback_reached.get() || observed.replace(true) {
            return;
        }
        for node in &callback_nodes {
            heap.payload_lease(node)
                .unwrap()
                .set_reference_elt(0, nil)
                .unwrap();
        }
        crate::sexp::gengc::full_gc();
        assert!(
            callback_nodes
                .iter()
                .all(crate::sexp::heap::CheckedNode::is_live)
        );
        assert!(callback_leaf.is_live());
    }));
    let mut copy = factory.wrap(unsafe { duplicate(source.as_raw()) }).unwrap();
    assert!(reached.get() && notified.get());
    assert!(source.try_vector_elt(0).unwrap().is_nil());
    for _ in 0..12 {
        copy = copy.try_vector_elt(0).unwrap();
    }
    assert_eq!(copy.try_integer_elt(0).unwrap(), 127);
    crate::sexp::gengc::full_gc();
    assert!(nodes[1..].iter().all(|node| !node.is_live()));
    assert!(
        !leaf_node.is_live(),
        "finished frames must release original roots"
    );
}

#[test]
fn owning_depth_provider_revocation_cleans_all_suspended_paths() {
    let session = RSession::new_for_gc_tests();
    let reached = Rc::new(Cell::new(false));
    let class = session
        .register_altrep_class(
            "revoked-deep-copy",
            CollectAtLeaf {
                reached: reached.clone(),
            },
        )
        .unwrap();
    let leaf = crate::sexp::altrep::AltrepBuilder::new(class)
        .build()
        .unwrap();
    let source = nested_vectors(&session, leaf, 12).into_owned().unwrap();
    let original = source.runtime_owner.as_ref().unwrap().clone();
    let holder = Rc::new(RefCell::new(Some(session)));
    let callback_holder = holder.clone();
    let callback_reached = reached.clone();
    let replacement = Rc::new(RefCell::new(None));
    let installed = replacement.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if !callback_reached.get() || callback_holder.borrow().is_none() {
            return;
        }
        drop(callback_holder.borrow_mut().take());
        *installed.borrow_mut() = Some(RSession::new_for_gc_tests());
    }));
    let state = DuplicationState::default();
    let outcome = crate::sexp::owner::with_runtime(&original, |access| {
        duplicate_iterative(source.clone(), 1, &state, access)
    });
    assert!(matches!(outcome, Err(SexpError::RootUnavailable)));
    assert!(reached.get());
    assert!(holder.borrow().is_none());
    assert!(state.active.borrow().is_empty());
    assert!(original.pin().is_err());
    let replacement = replacement.borrow();
    let replacement = replacement.as_ref().unwrap();
    let factory = replacement.owner_token().unwrap().node_factory();
    assert!(factory.wrap(source.as_raw()).is_err());
    replacement.with_active(|| {
        let value = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(131) })
            .unwrap();
        assert_eq!(
            factory
                .wrap(unsafe { duplicate(value.as_raw()) })
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            131
        );
    });
}
