//! GNU external pointers preserve allocation identity under duplication.

use super::{IS_S4_OBJECT, duplicate, lazy_duplicate, shallow_duplicate};
use crate::sexp::{
    accessors::{OBJECT, SET_ATTRIB, SET_OBJECT, SET_S4_OBJECT},
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement, is_altrep},
    ffi::SEXPTYPE,
    gengc, memory,
    object::{PairlistBuilder, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};

struct CollectingChild {
    value: i32,
    reads: Rc<Cell<usize>>,
}

impl AltrepClass for CollectingChild {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }

    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }

    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.reads.set(self.reads.get() + 1);
        context.gc()?;
        Ok(AltrepElement::Integer(self.value))
    }
}

fn identity_and_graph_lifetime(deep: bool) {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let reads = [
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
    ];
    let children: Vec<_> = reads
        .iter()
        .enumerate()
        .map(|(index, reads)| {
            let class = session
                .register_altrep_class(
                    &format!("identity-child-{index}"),
                    CollectingChild {
                        value: 41 + index as i32,
                        reads: reads.clone(),
                    },
                )
                .unwrap();
            AltrepBuilder::new(class).build().unwrap()
        })
        .collect();
    let child_nodes: Vec<_> = children
        .iter()
        .map(|child| child.allocation().unwrap().clone())
        .collect();
    let mut builder = PairlistBuilder::from_factory(factory.clone());
    builder.push(children[2].clone(), None).unwrap();
    let attributes = builder.finish().unwrap();
    let attribute_node = attributes.allocation().unwrap().clone();
    let source = factory
        .wrap(unsafe {
            crate::mainutils::memory_main::R_MakeExternalPtr(
                std::ptr::null_mut(),
                children[1].as_raw(),
                children[0].as_raw(),
            )
        })
        .unwrap();
    unsafe {
        SET_ATTRIB(source.as_raw(), attributes.as_raw());
        SET_OBJECT(source.as_raw(), 1);
        SET_S4_OBJECT(source.as_raw());
    }
    let source_node = source.allocation().unwrap().clone();
    let notifications = Rc::new(Cell::new(0));
    let observed = notifications.clone();
    gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
    }));
    // This fixture has no outstanding arena loan and retains its original session.
    let before = session.with_active(|| unsafe { memory::with_arena(|arena| arena.node_count()) });
    let protection_count = crate::sexp::protect::R_ProtectCount();
    let pointer = unsafe {
        if deep {
            duplicate(source.as_raw())
        } else {
            shallow_duplicate(source.as_raw())
        }
    };
    let alias = factory.wrap(pointer).unwrap();
    assert_eq!(alias.allocation().unwrap(), &source_node);
    assert_eq!(pointer, source.as_raw());
    assert_eq!(
        session.with_active(|| unsafe { memory::with_arena(|arena| arena.node_count()) }),
        before
    );
    assert_eq!(
        notifications.get(),
        0,
        "identity duplication cannot trigger allocating callbacks"
    );
    assert!(
        reads.iter().all(|count| count.get() == 0),
        "duplication must not materialize children"
    );
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection_count);
    assert_eq!(unsafe { lazy_duplicate(source.as_raw()) }, pointer);
    assert_eq!(
        alias.try_extprot().unwrap().allocation().unwrap(),
        &child_nodes[0]
    );
    assert_eq!(
        alias.try_extptr_tag().unwrap().allocation().unwrap(),
        &child_nodes[1]
    );
    assert_eq!(
        alias.try_attrib().unwrap().allocation().unwrap(),
        &attribute_node
    );
    assert_eq!(unsafe { OBJECT(alias.as_raw()) }, 1);
    assert_ne!(unsafe { IS_S4_OBJECT(alias.as_raw()) }, 0);

    // Mutations through either owning view affect the same GNU object.
    unsafe {
        crate::mainutils::memory_main::R_SetExternalPtrProtected(
            alias.as_raw(),
            children[1].as_raw(),
        );
    }
    assert_eq!(
        source.try_extprot().unwrap().allocation().unwrap(),
        &child_nodes[1]
    );
    unsafe {
        crate::mainutils::memory_main::R_SetExternalPtrProtected(
            source.as_raw(),
            children[0].as_raw(),
        );
    }
    // Remove every incidental graph root before actual collecting provider reads.
    drop(source);
    drop(attributes);
    drop(children);
    gengc::full_gc();
    assert!(source_node.is_live());
    assert!(attribute_node.is_live());
    assert!(child_nodes.iter().all(|node| node.is_live()));
    {
        let saved = [
            alias.try_extprot().unwrap(),
            alias.try_extptr_tag().unwrap(),
            alias.try_attrib().unwrap().try_car().unwrap(),
        ];
        for (index, child) in saved.iter().enumerate() {
            assert!(is_altrep(child));
            assert_eq!(child.try_integer_elt(0).unwrap(), 41 + index as i32);
            assert!(reads[index].get() > 0);
        }
    }
    assert!(
        notifications.get() > 0,
        "lifetime assertions require real collection"
    );
    drop(alias);
    gengc::full_gc();
    assert!(!source_node.is_live());
    assert!(!attribute_node.is_live());
    assert!(child_nodes.iter().all(|node| !node.is_live()));
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection_count);
}

#[test]
fn external_identity_deep_copy_keeps_one_collectible_graph() {
    identity_and_graph_lifetime(true);
}

#[test]
fn external_identity_shallow_copy_keeps_one_collectible_graph() {
    identity_and_graph_lifetime(false);
}

struct Resource(Rc<Cell<usize>>);
impl Drop for Resource {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn external_identity_explicit_resource_close_revokes_every_alias_once() {
    for deep in [false, true] {
        let session = RSession::new_for_gc_tests();
        let factory = session.owner_token().unwrap().node_factory();
        let drops = Rc::new(Cell::new(0));
        let resource = Rc::new(Resource(drops.clone()));
        let weak = Rc::downgrade(&resource);
        let address = Rc::as_ptr(&resource).cast_mut().cast();
        let source = factory
            .allocate(|arena| {
                let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                let node = arena.node_token(pointer)?;
                let heap = arena.heap_identity();
                let mut header = heap.node_snapshot(&node)?;
                header.data.extptr_mut().address = address;
                heap.replace_node(&node, header)?;
                heap.attach_resource(&node, resource.clone())?;
                Some(pointer)
            })
            .unwrap();
        drop(resource);
        let pointer = unsafe {
            if deep {
                duplicate(source.as_raw())
            } else {
                shallow_duplicate(source.as_raw())
            }
        };
        let alias = factory.wrap(pointer).unwrap();
        let node = source.allocation().unwrap().clone();
        let heap = node.heap_identity();
        assert_eq!(alias.allocation().unwrap(), &node);
        assert!(
            heap.resource::<Resource>(alias.allocation().unwrap())
                .is_some()
        );
        unsafe {
            crate::mainutils::memory_main::R_ClearExternalPtr(alias.as_raw());
        }
        assert!(source.try_extptr_ptr().unwrap().is_null());
        drop(heap.take_resource(alias.allocation().unwrap()).unwrap());
        assert_eq!(drops.get(), 1);
        assert!(weak.upgrade().is_none());
        assert!(heap.resource::<Resource>(&node).is_none());
        drop(source);
        gengc::full_gc();
        assert!(
            node.is_live(),
            "the remaining alias still owns this allocation"
        );
        assert_eq!(drops.get(), 1);
        drop(alias);
        gengc::full_gc();
        assert!(!node.is_live());
        assert_eq!(drops.get(), 1);
    }
}

#[test]
fn external_identity_last_owned_alias_releases_resource_after_session_shutdown() {
    let mut session = RSession::new_for_gc_tests();
    let drops = Rc::new(Cell::new(0));
    let (alias, node, heap, weak, address) = {
        let factory = session.owner_token().unwrap().node_factory();
        let resource = Rc::new(Resource(drops.clone()));
        let weak = Rc::downgrade(&resource);
        let address = Rc::as_ptr(&resource).cast_mut().cast();
        let source = factory
            .allocate(|arena| {
                let pointer = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
                let node = arena.node_token(pointer)?;
                let heap = arena.heap_identity();
                let mut header = heap.node_snapshot(&node)?;
                header.data.extptr_mut().address = address;
                heap.replace_node(&node, header)?;
                heap.attach_resource(&node, resource.clone())?;
                Some(pointer)
            })
            .unwrap();
        drop(resource);
        let alias = factory
            .wrap(unsafe { duplicate(source.as_raw()) })
            .unwrap()
            .into_owned()
            .unwrap();
        let node = source.allocation().unwrap().clone();
        assert_eq!(alias.allocation().unwrap(), &node);
        let heap = node.heap_identity();
        drop(source);
        (alias, node, heap, weak, address)
    };
    session.close();
    drop(session);
    assert_eq!(drops.get(), 0);
    assert!(node.is_live());
    assert_eq!(alias.try_extptr_ptr().unwrap(), address);
    assert!(
        alias.pin_runtime().is_err(),
        "physical ownership cannot restore closed runtime authority"
    );
    drop(alias);
    assert_eq!(drops.get(), 1);
    assert!(weak.upgrade().is_none());
    assert!(heap.node_snapshot(&node).is_none());
    assert!(!node.is_live());
}
