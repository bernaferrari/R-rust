use crate::sexp::{
    RSession, SEXPTYPE,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::SexpResult,
};
use std::{cell::Cell, rc::Rc};

struct Provider {
    size: i64,
    calls: Rc<Cell<usize>>,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.size)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        context.gc()?;
        (self.callback)();
        Ok(AltrepElement::Integer(i as i32 + 1))
    }
}

struct CharacterProvider {
    callback: Rc<dyn Fn()>,
}

impl AltrepClass for CharacterProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        context.gc()?;
        (self.callback)();
        Ok(AltrepElement::String(context.string(if i == 0 {
            "é"
        } else {
            "日本"
        })?))
    }
}

#[test]
fn atomic_names_short_character_altrep_gc_and_revocation() {
    for revoke in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| {
            let owner = session.owner_token().unwrap();
            let target = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 3)))
                .unwrap();
            let class = session
                .register_altrep_class(
                    "short-character-names",
                    CharacterProvider {
                        callback: Rc::new(move || {
                            if revoke {
                                unsafe {
                                    crate::sexp::instance::revoke_instance_availability(instance)
                                }
                            }
                        }),
                    },
                )
                .unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::setAttrib(target.as_raw(), super::R_NamesSymbol(), input.as_raw());
            }));
            if revoke {
                assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
                assert!(target.try_attrib().unwrap().is_nil());
            } else {
                result.unwrap();
                owner.full_gc().unwrap();
                let names = target.try_attrib().unwrap().try_car().unwrap();
                assert!(
                    names
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq("é".as_bytes())
                        .unwrap()
                );
                assert!(
                    names
                        .try_string_elt(1)
                        .unwrap()
                        .try_char_eq("日本".as_bytes())
                        .unwrap()
                );
                assert!(names.try_string_elt(2).unwrap().is_na_string());
            }
        });
    }
}

fn ownership_case(mode: u8, eval_path: bool, input_size: i64) {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let target = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
            .unwrap()
            .into_owned()
            .unwrap();
        let mode_cell = Rc::new(Cell::new(mode));
        let callback_mode = mode_cell.clone();
        let calls = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class(
                "names-owner",
                Provider {
                    size: input_size,
                    calls: calls.clone(),
                    callback: Rc::new(move || match callback_mode.get() {
                        1 => {
                            let other = RSession::new_for_gc_tests();
                            other.owner_token().unwrap().full_gc().unwrap();
                        }
                        2 => panic!("names provider unwind"),
                        3 => unsafe {
                            crate::sexp::instance::revoke_instance_availability(instance);
                        },
                        _ => {}
                    }),
                },
            )
            .unwrap();
        let input = AltrepBuilder::new(class).build().unwrap();
        let name = unsafe { super::R_NamesSymbol() };
        let invoke = || unsafe {
            if eval_path {
                crate::eval::attrib_core::setAttrib(target.as_raw(), name, input.as_raw());
            } else {
                super::setAttrib(target.as_raw(), name, input.as_raw());
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(invoke));
        if mode == 3 {
            let payload = result.unwrap_err();
            assert!(payload.is::<crate::sexp::context::RError>());
            assert!(!session.is_active());
            // The retained value keeps its physical allocation readable after
            // revocation; no names may have been published to it.
            assert!(target.try_attrib().unwrap().is_nil());
        } else if mode == 2 {
            assert!(result.unwrap_err().is::<&str>());
            assert!(target.try_attrib().unwrap().is_nil());
            mode_cell.set(0);
            invoke();
        } else {
            result.unwrap();
            assert_eq!(calls.get(), input_size as usize);
            owner.full_gc().unwrap();
            let names = target
                .try_attrib()
                .unwrap()
                .try_car()
                .unwrap()
                .into_owned()
                .unwrap();
            assert_eq!(names.typeof_(), SEXPTYPE::STRSXP);
            for (i, expected) in [b"1", b"2", b"3"]
                .into_iter()
                .enumerate()
                .take(input_size as usize)
            {
                assert!(
                    names
                        .try_string_elt(i as i64)
                        .unwrap()
                        .try_char_eq(expected)
                        .unwrap()
                );
            }
            for i in input_size..3 {
                assert!(names.try_string_elt(i).unwrap().is_na_string());
            }
        }
    });
}
#[test]
fn atomic_names_gc_reentry_panic_and_revocation() {
    for eval in [false, true] {
        for mode in 0..4 {
            for input_size in [2, 3] {
                ownership_case(mode, eval, input_size);
            }
        }
    }
}

#[test]
fn atomic_names_matrix_dimension_removal_during_coercion_does_not_publish() {
    use crate::sexp::object::SexpMut;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap()
        .into_owned()
        .unwrap();
    let dimensions = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap()
        .into_owned()
        .unwrap();
    let mut dimensions = SexpMut::try_from_checked(dimensions).unwrap();
    dimensions.try_set_integer_elt(0, 1).unwrap();
    dimensions.try_set_integer_elt(1, 3).unwrap();
    let dimensions = dimensions.freeze();
    unsafe { super::setAttrib(target.as_raw(), super::R_DimSymbol(), dimensions.as_raw()) };
    let callback_target = target.clone();
    let nil = factory.nil().into_owned().unwrap();
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "matrix-names-dim-change",
            Provider {
                size: 2,
                calls,
                callback: Rc::new(move || {
                    SexpMut::try_from_checked(callback_target.clone())
                        .unwrap()
                        .try_set_attribute(&nil)
                        .unwrap();
                }),
            },
        )
        .unwrap();
    let input = AltrepBuilder::new(class).build().unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::setAttrib(target.as_raw(), super::R_NamesSymbol(), input.as_raw());
    }))
    .unwrap_err();
    assert!(failure.is::<crate::sexp::context::RError>());
    assert!(target.try_attrib().unwrap().is_nil());
}

#[test]
fn atomic_names_matrix_padding_denial_preserves_dimensions() {
    use crate::sexp::{memory::ArenaBudget, object::SexpMut};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 10000)))
        .unwrap();
    let dimensions = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    let mut dimensions = SexpMut::try_from_checked(dimensions).unwrap();
    dimensions.try_set_integer_elt(0, 100).unwrap();
    dimensions.try_set_integer_elt(1, 100).unwrap();
    let dimensions = dimensions.freeze();
    let input = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 0)))
        .unwrap();
    unsafe { super::setAttrib(target.as_raw(), super::R_DimSymbol(), dimensions.as_raw()) };
    let original_attributes = target.try_attrib().unwrap().into_owned().unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    owner.full_gc().unwrap();
    let resident = owner
        .with_arena(|arena| arena.total_bytes_allocated())
        .unwrap();
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::new(resident + 4096, usize::MAX)))
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::setAttrib(target.as_raw(), name, input.as_raw());
    }))
    .unwrap_err();
    let error = failure.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert_eq!(target.try_attrib().unwrap(), original_attributes);
    assert_eq!(original_attributes.try_car().unwrap(), dimensions);
    assert!(original_attributes.try_cdr().unwrap().is_nil());
    owner
        .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn atomic_names_denial_preserves_typed_error_and_target() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 10000)))
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "names-denial",
            Provider {
                size: 10000,
                calls: calls.clone(),
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 4096, usize::MAX)))
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::setAttrib(target.as_raw(), name, source.as_raw());
    }))
    .unwrap_err();
    let error = failure.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert_eq!(calls.get(), 0);
    assert!(target.try_attrib().unwrap().is_nil());
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn atomic_names_padding_denial_preserves_target() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 10000)))
        .unwrap();
    let input = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 0)))
        .unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 4096, usize::MAX)))
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::setAttrib(target.as_raw(), name, input.as_raw());
    }))
    .unwrap_err();
    let error = failure.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert!(target.try_attrib().unwrap().is_nil());
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn atomic_names_admits_detached_target_before_provider_gc() {
    use crate::sexp::object::SexpMut;
    use std::cell::RefCell;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let source_slot = Rc::new(RefCell::new(None::<crate::sexp::object::Sexp<'static>>));
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap()
        .into_owned()
        .unwrap();
    let identity = target.allocation().unwrap().clone();
    let callback_identity = identity.clone();
    let callback_source = source_slot.clone();
    let nil = factory.nil().into_owned().unwrap();
    let calls = Rc::new(Cell::new(0));
    let callback_calls = calls.clone();
    let class = session
        .register_altrep_class(
            "names-detached",
            Provider {
                size: 3,
                calls,
                callback: Rc::new(move || {
                    if callback_calls.get() == 1 {
                        let source = callback_source.borrow().as_ref().unwrap().clone();
                        let mut source = SexpMut::try_from_checked(source).unwrap();
                        source.try_set_pairlist_car(&nil).unwrap();
                        source.try_set_pairlist_cdr(&nil).unwrap();
                        crate::sexp::gengc::full_gc();
                        assert!(callback_identity.is_live());
                    }
                }),
            },
        )
        .unwrap();
    let input = AltrepBuilder::new(class).build().unwrap();
    let tail = factory
        .pairlist_cell(&target, &factory.nil(), &factory.nil())
        .unwrap();
    let source = factory
        .pairlist_cell(&input, &tail, &factory.nil())
        .unwrap()
        .into_owned()
        .unwrap();
    *source_slot.borrow_mut() = Some(source);
    let target_raw = target.as_raw();
    let input_raw = input.as_raw();
    let input_identity = input.allocation().unwrap().clone();
    drop(target);
    drop(input);
    drop(tail);
    let target = unsafe {
        super::setAttrib(target_raw, super::R_NamesSymbol(), input_raw);
        owner.sexp(target_raw).unwrap().into_owned().unwrap()
    };
    assert!(
        target
            .try_attrib()
            .unwrap()
            .try_tag_name_eq(b"names")
            .unwrap()
    );
    owner.full_gc().unwrap();
    assert!(!input_identity.is_live());
    assert!(identity.is_live());
    drop(target);
    owner.full_gc().unwrap();
    assert!(!identity.is_live());
}

#[test]
fn atomic_names_publication_appends_to_chain_replaced_during_cell_allocation() {
    use crate::sexp::ffi::NodeBody;
    for add_dim in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
        let token = session.owner_token().unwrap();
        let owner = token.weak_owner().unwrap();
        let factory = token.node_factory();
        let object = factory.allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3))).unwrap().into_owned().unwrap();
        let input = factory.allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3))).unwrap().into_owned().unwrap();
        let old_tag = unsafe { token.sexp(super::Rf_install(c"old".as_ptr())) }
            .unwrap()
            .into_owned()
            .unwrap();
        let current_tag = unsafe { token.sexp(super::Rf_install(if add_dim { c"dim".as_ptr() } else { c"current".as_ptr() })) }
            .unwrap()
            .into_owned()
            .unwrap();
        let old = factory
            .pairlist_cell(&factory.nil(), &factory.nil(), &old_tag)
            .unwrap()
            .into_owned()
            .unwrap();
        let nil = factory.nil();
        let current = factory
            .pairlist_cell(if add_dim { &input } else { &nil }, &nil, &current_tag)
            .unwrap()
            .into_owned()
            .unwrap();
        crate::sexp::object::SexpMut::try_from_checked(object.clone())
            .unwrap()
            .try_set_attribute(&old)
            .unwrap();
        let identity = object.allocation().unwrap().clone();
        let names_name = unsafe { token.sexp(super::R_NamesSymbol()) }
            .unwrap()
            .into_owned()
            .unwrap();
        let names_link = names_name.link_in(&identity.heap_identity()).unwrap();
        let domain = crate::sexp::owner::with_runtime(&owner, |access| access.domain()).unwrap();
        let replaced = Rc::new(Cell::new(false));
        let callback_replaced = replaced.clone();
        let callback_current = current.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if callback_replaced.get() {
                return;
            }
            // Constructors notify only after the new cell is initialized.
            // Finish this arena inspection before changing edges or collecting.
            let names_cell_exists =
                crate::sexp::instance::with_required_current_instance(|runtime| unsafe {
                    (*runtime).arena.active_nodes().any(|pointer| {
                        let Some(node) = (*runtime).arena.node_token(pointer) else {
                            return false;
                        };
                        let Some(header) = node.heap_identity().node_snapshot(&node) else {
                            return false;
                        };
                        header.sxpinfo.type_of() == SEXPTYPE::LISTSXP
                            && matches!(header.data, NodeBody::List(body) if body.tagval == names_link)
                    })
                });
            if !names_cell_exists {
                return;
            }
            callback_replaced.set(true);
            let pointer = identity
                .heap_identity()
                .projection_of_link(identity.link().unwrap())
                .unwrap();
            let target = domain.wrap(pointer).unwrap();
            crate::sexp::object::SexpMut::try_from_checked(target)
                .unwrap()
                .try_set_attribute(&callback_current)
                .unwrap();
            crate::sexp::gengc::full_gc();
        }));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            super::setAttrib(object.as_raw(), super::R_NamesSymbol(), input.as_raw());
        }));
        unsafe { crate::mainutils::memory_main::R_gc_torture(0, 0, 0); }
        if add_dim {
            assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
            assert!(replaced.get());
            assert_eq!(object.try_attrib().unwrap(), current);
            assert!(current.try_cdr().unwrap().is_nil());
            return;
        }
        result.unwrap();
        assert!(
            replaced.get(),
            "replacement happened during initialized cell allocation"
        );
        assert_eq!(object.try_attrib().unwrap(), current);
        assert!(
            old.try_cdr().unwrap().is_nil(),
            "detached tail must remain unchanged"
        );
        let added = current.try_cdr().unwrap();
        assert!(added.try_tag_name_eq(b"names").unwrap());
        assert!(added.try_cdr().unwrap().is_nil());
        assert!(added.try_car().unwrap().try_string_elt(1).unwrap().try_char_eq(b"0").unwrap());
    });
    }
}

#[test]
fn atomic_names_checked_header_probe_rejects_foreign_singleton_and_stale() {
    let other = RSession::new_for_gc_tests();
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let names = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 3)))
        .unwrap();
    let foreign = other.with_active(|| {
        other
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 3)))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let domain = factory.domain();
    let name = unsafe { super::R_NamesSymbol() };
    assert!(domain.has_equal_plain_character_input(target.as_raw(), names.as_raw(), name));
    assert!(!domain.has_equal_plain_character_input(target.as_raw(), target.as_raw(), name));
    assert!(!domain.has_equal_plain_character_input(target.as_raw(), foreign.as_raw(), name));
    assert!(!domain.has_equal_plain_character_input(foreign.as_raw(), names.as_raw(), name));
    assert!(!domain.has_equal_plain_character_input(factory.nil().as_raw(), names.as_raw(), name));
    let stale = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 2)))
        .unwrap();
    assert!(!domain.has_equal_plain_character_input(target.as_raw(), stale.as_raw(), name));
    let stale_raw = stale.as_raw();
    let allocation = stale.allocation().unwrap().clone();
    drop(stale);
    owner.full_gc().unwrap();
    assert!(!allocation.is_live());
    assert!(!domain.has_equal_plain_character_input(target.as_raw(), stale_raw, name));
}

#[test]
fn atomic_names_rejects_foreign_and_revoked_values_before_headers() {
    let mut foreign = RSession::new_for_gc_tests();
    let factory = foreign.owner_token().unwrap().node_factory();
    let value = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let foreign_string = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 3)))
        .unwrap()
        .into_owned()
        .unwrap();
    let foreign_raw = value.as_raw();
    let current = RSession::new_for_gc_tests();
    let factory = current.owner_token().unwrap().node_factory();
    let local = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let local_string = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 3)))
        .unwrap();
    for revoked in [false, true] {
        if revoked {
            foreign.close();
        }
        current.with_active(|| {
            for eval in [false, true] {
                for (object, input) in [
                    (foreign_raw, local.as_raw()),
                    (local.as_raw(), foreign_raw),
                    (foreign_raw, local_string.as_raw()),
                    (local.as_raw(), foreign_string.as_raw()),
                ] {
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                            let name = super::R_NamesSymbol();
                            if eval {
                                crate::eval::attrib_core::setAttrib(object, name, input);
                            } else {
                                super::setAttrib(object, name, input);
                            }
                        }))
                        .unwrap_err();
                    assert!(result.is::<crate::sexp::context::RError>());
                    assert!(local.try_attrib().unwrap().is_nil());
                }
            }
        });
    }
}

#[test]
#[ignore = "manual matched baseline/candidate timing control"]
fn atomic_names_assignment_benchmark() {
    use std::time::Instant;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let object = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let integer = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let string = factory
        .wrap(unsafe { crate::mainutils::coerce::coerceToString(integer.as_raw()) })
        .unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    for eval in [false, true] {
        for (kind, value) in [
            ("character", string.as_raw()),
            ("integer", integer.as_raw()),
        ] {
            for repeat in 0..5 {
                owner.full_gc().unwrap();
                let start = Instant::now();
                for _ in 0..10000 {
                    unsafe {
                        if eval {
                            crate::eval::attrib_core::setAttrib(object.as_raw(), name, value);
                        } else {
                            super::setAttrib(object.as_raw(), name, value);
                        }
                    }
                }
                println!(
                    "names_timing eval={eval} kind={kind} repeat={repeat} iterations=10000 ns={}",
                    start.elapsed().as_nanos()
                );
            }
        }
    }
}

#[test]
fn atomic_names_leaves_classed_input_and_pads_short_input() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let source = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    unsafe {
        let class = crate::sexp::constructors::Rf_mkString(c"factor".as_ptr());
        let class = factory.wrap(class).unwrap();
        super::setAttrib(source.as_raw(), super::R_ClassSymbol(), class.as_raw());
        assert!(!super::try_set_atomic_names(
            target.as_raw(),
            name,
            source.as_raw()
        ));
        super::setAttrib(target.as_raw(), name, source.as_raw());
    }
    assert_eq!(target.try_attrib().unwrap().try_car().unwrap(), source);
    assert!(source.is_object());
    let short = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 2)))
        .unwrap();
    unsafe {
        assert!(super::try_set_atomic_names(
            target.as_raw(),
            name,
            short.as_raw()
        ));
    }
    let names = target.try_attrib().unwrap().try_car().unwrap();
    assert_eq!(names.len(), 3);
    assert!(names.try_string_elt(2).unwrap().is_na_string());
}

#[test]
fn one_dimensional_names_survive_gc_and_reject_revocation() {
    use crate::sexp::object::SexpMut;
    for revoke in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let target = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
                .unwrap();
            let dimension = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let mut dimension = SexpMut::try_from_checked(dimension).unwrap();
            dimension.try_set_integer_elt(0, 3).unwrap();
            let dimension = dimension.freeze();
            unsafe { super::setAttrib(target.as_raw(), super::R_DimSymbol(), dimension.as_raw()) };
            let original = target.try_attrib().unwrap().into_owned().unwrap();
            let calls = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class(
                    "one-dim-names",
                    Provider {
                        size: 2,
                        calls: calls.clone(),
                        callback: Rc::new(move || {
                            if revoke {
                                unsafe {
                                    crate::sexp::instance::revoke_instance_availability(instance)
                                }
                            }
                        }),
                    },
                )
                .unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::setAttrib(target.as_raw(), super::R_NamesSymbol(), input.as_raw());
            }));
            if revoke {
                assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
                assert_eq!(target.try_attrib().unwrap(), original);
                assert!(original.try_cdr().unwrap().is_nil());
            } else {
                result.unwrap();
                assert_eq!(calls.get(), 2);
                owner.full_gc().unwrap();
                let dimnames = original.try_cdr().unwrap();
                assert!(dimnames.try_tag_name_eq(b"dimnames").unwrap());
                let names = dimnames.try_car().unwrap().try_vector_elt(0).unwrap();
                assert!(names.try_string_elt(0).unwrap().try_char_eq(b"1").unwrap());
                assert!(names.try_string_elt(1).unwrap().try_char_eq(b"2").unwrap());
                assert!(names.try_string_elt(2).unwrap().is_na_string());
            }
        });
    }
}

#[test]
fn one_dimensional_names_denial_preserves_existing_dimensions() {
    use crate::sexp::{memory::ArenaBudget, object::SexpMut};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 10000)))
        .unwrap();
    let dimension = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut dimension = SexpMut::try_from_checked(dimension).unwrap();
    dimension.try_set_integer_elt(0, 10000).unwrap();
    let dimension = dimension.freeze();
    let input = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 0)))
        .unwrap();
    unsafe { super::setAttrib(target.as_raw(), super::R_DimSymbol(), dimension.as_raw()) };
    let original = target.try_attrib().unwrap().into_owned().unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 4096, usize::MAX)))
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::setAttrib(target.as_raw(), name, input.as_raw());
    }))
    .unwrap_err();
    let error = failure.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert_eq!(target.try_attrib().unwrap(), original);
    assert_eq!(original.try_car().unwrap(), dimension);
    assert!(original.try_cdr().unwrap().is_nil());
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn one_dimensional_names_fast_fallback_reuses_existing_vector_attribute() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
        .unwrap();
    let old = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 3)))
        .unwrap();
    let replacement = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 3)))
        .unwrap();
    let name = unsafe { super::R_NamesSymbol() };
    unsafe { super::setAttrib(target.as_raw(), name, old.as_raw()) };
    assert!(factory.domain().has_equal_plain_character_input(
        target.as_raw(),
        replacement.as_raw(),
        name
    ));
    let attribute = target.try_attrib().unwrap().into_owned().unwrap();
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident, usize::MAX)))
        .unwrap();
    unsafe { super::setAttrib(target.as_raw(), name, replacement.as_raw()) };
    assert_eq!(target.try_attrib().unwrap(), attribute);
    assert_eq!(attribute.try_car().unwrap(), replacement);
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
    let dimensions = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    unsafe { super::setAttrib(target.as_raw(), super::R_DimSymbol(), dimensions.as_raw()) };
    assert!(!factory.domain().has_equal_plain_character_input(
        target.as_raw(),
        replacement.as_raw(),
        name
    ));
}

#[test]
fn names_replacement_plain_copy_survives_gc_and_rejects_revocation() {
    for revoke in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let target = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 2)))
                .unwrap();
            let marker = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let class = session
                .register_altrep_class(
                    "plain-names-replacement",
                    CharacterProvider {
                        callback: Rc::new(move || {
                            if revoke {
                                unsafe {
                                    crate::sexp::instance::revoke_instance_availability(instance)
                                }
                            }
                        }),
                    },
                )
                .unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            unsafe {
                super::setAttrib(
                    input.as_raw(),
                    super::Rf_install(c"marker".as_ptr()),
                    marker.as_raw(),
                )
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::plain_names_replacement(target.as_raw(), input.as_raw())
            }));
            if revoke {
                assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
                assert!(target.try_attrib().unwrap().is_nil());
            } else {
                let result = result.unwrap().unwrap();
                owner.full_gc().unwrap();
                assert!(result.try_attrib().unwrap().is_nil());
                assert!(
                    result
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq("é".as_bytes())
                        .unwrap()
                );
                assert!(
                    result
                        .try_string_elt(1)
                        .unwrap()
                        .try_char_eq("日本".as_bytes())
                        .unwrap()
                );
                assert!(!input.try_attrib().unwrap().is_nil());
            }
        });
    }
}

#[test]
fn names_replacement_plain_copy_denial_preserves_source_and_target() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1024)))
        .unwrap();
    let input = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1024)))
        .unwrap();
    let marker = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    unsafe {
        super::setAttrib(
            input.as_raw(),
            super::Rf_install(c"marker".as_ptr()),
            marker.as_raw(),
        )
    };
    let original = input.try_attrib().unwrap().into_owned().unwrap();
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 4096, usize::MAX)))
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::plain_names_replacement(target.as_raw(), input.as_raw())
    }))
    .unwrap_err();
    let error = failure.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert_eq!(input.try_attrib().unwrap(), original);
    assert!(target.try_attrib().unwrap().is_nil());
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn names_replacement_attribute_probe_rejects_foreign_singleton_and_stale() {
    let other = RSession::new_for_gc_tests();
    let foreign = other
        .owner_token()
        .unwrap()
        .node_factory()
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
        .unwrap()
        .into_owned()
        .unwrap();
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let _pin = owner.pin().unwrap();
    let factory = owner.node_factory();
    let local = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
        .unwrap();
    let marker = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let domain = factory.domain();
    assert!(domain.has_no_attributes(local.as_raw()));
    assert!(!domain.has_no_attributes(foreign.as_raw()));
    assert!(!domain.has_no_attributes(factory.nil().as_raw()));
    unsafe {
        super::setAttrib(
            local.as_raw(),
            super::Rf_install(c"marker".as_ptr()),
            marker.as_raw(),
        )
    };
    assert!(!domain.has_no_attributes(local.as_raw()));
    let stale = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))
        .unwrap();
    let raw = stale.as_raw();
    let allocation = stale.allocation().unwrap().clone();
    drop(stale);
    owner.full_gc().unwrap();
    assert!(!allocation.is_live());
    assert!(!domain.has_no_attributes(raw));
}

#[test]
fn padded_names_metadata_survives_gc_and_rejects_revocation() {
    use crate::sexp::object::SexpMut;
    for revoke in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let target = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
                .unwrap();
            let input = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
                .unwrap();
            let mut input = SexpMut::try_from_checked(input).unwrap();
            input
                .try_set_string_elt(0, factory.character("A").unwrap())
                .unwrap();
            input
                .try_set_string_elt(1, factory.character("B").unwrap())
                .unwrap();
            let input = input.freeze();
            let class = session
                .register_altrep_class(
                    "padded-names-metadata",
                    CharacterProvider {
                        callback: Rc::new(move || {
                            if revoke {
                                unsafe {
                                    crate::sexp::instance::revoke_instance_availability(instance)
                                }
                            }
                        }),
                    },
                )
                .unwrap();
            let nested = AltrepBuilder::new(class).build().unwrap();
            unsafe { super::setAttrib(input.as_raw(), super::R_NamesSymbol(), nested.as_raw()) };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::setAttrib(target.as_raw(), super::R_NamesSymbol(), input.as_raw());
            }));
            if revoke {
                assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
                assert!(target.try_attrib().unwrap().is_nil());
            } else {
                result.unwrap();
                owner.full_gc().unwrap();
                let names = target.try_attrib().unwrap().try_car().unwrap();
                assert!(names.try_string_elt(2).unwrap().is_na_string());
                let nested = names.try_attrib().unwrap().try_car().unwrap();
                assert!(
                    nested
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq("é".as_bytes())
                        .unwrap()
                );
                assert!(
                    nested
                        .try_string_elt(1)
                        .unwrap()
                        .try_char_eq("日本".as_bytes())
                        .unwrap()
                );
                assert!(nested.try_string_elt(2).unwrap().try_char_eq(b"").unwrap());
            }
        });
    }
}

#[test]
fn padded_names_metadata_denial_does_not_publish() {
    use crate::sexp::{memory::ArenaBudget, object::SexpMut};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let target = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1024)))
        .unwrap();
    let input = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
        .unwrap();
    let nested = factory
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
        .unwrap();
    let mut nested = SexpMut::try_from_checked(nested).unwrap();
    nested
        .try_set_string_elt(0, factory.character("é").unwrap())
        .unwrap();
    nested
        .try_set_string_elt(1, factory.character("日本").unwrap())
        .unwrap();
    let nested = nested.freeze();
    let name = unsafe { super::R_NamesSymbol() };
    unsafe { super::setAttrib(input.as_raw(), name, nested.as_raw()) };
    let original = input.try_attrib().unwrap().into_owned().unwrap();
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 12000, usize::MAX)))
        .unwrap();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::setAttrib(target.as_raw(), name, input.as_raw());
    }))
    .unwrap_err();
    let error = failure.downcast::<crate::sexp::context::RError>().unwrap();
    assert_eq!(
        error.message,
        "R allocation failed: memory or node budget exceeded"
    );
    assert!(target.try_attrib().unwrap().is_nil());
    assert_eq!(input.try_attrib().unwrap(), original);
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

struct DimnamesProvider {
    callback: Rc<dyn Fn()>,
    calls: Rc<Cell<usize>>,
}
impl AltrepClass for DimnamesProvider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::VECSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        context.gc()?;
        (self.callback)();
        let value = context.alloc_vector(SEXPTYPE::STRSXP, 2)?;
        let mut value = crate::sexp::object::SexpMut::try_from_checked(value)?;
        value.try_set_string_elt(0, context.string("é")?)?;
        value.try_set_string_elt(1, context.string("日本")?)?;
        Ok(AltrepElement::List(value.freeze()))
    }
}

#[test]
fn padded_names_metadata_dimnames_provider_gc_and_revocation() {
    use crate::sexp::object::SexpMut;
    for revoke in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let target = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
                .unwrap();
            let input = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
                .unwrap();
            let dimension = factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let mut dimension = SexpMut::try_from_checked(dimension).unwrap();
            dimension.try_set_integer_elt(0, 2).unwrap();
            let dimension = dimension.freeze();
            let calls = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class(
                    "padded-dimnames-provider",
                    DimnamesProvider {
                        calls: calls.clone(),
                        callback: Rc::new(move || {
                            if revoke {
                                unsafe {
                                    crate::sexp::instance::revoke_instance_availability(instance)
                                }
                            }
                        }),
                    },
                )
                .unwrap();
            let dimnames = AltrepBuilder::new(class).build().unwrap();
            unsafe {
                super::setAttrib(input.as_raw(), super::R_DimSymbol(), dimension.as_raw());
                super::setAttrib(input.as_raw(), super::R_DimNamesSymbol(), dimnames.as_raw());
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::setAttrib(target.as_raw(), super::R_NamesSymbol(), input.as_raw());
            }));
            if revoke {
                assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
                assert!(target.try_attrib().unwrap().is_nil());
            } else {
                result.unwrap();
                assert_eq!(calls.get(), 1);
                owner.full_gc().unwrap();
                let names = target.try_attrib().unwrap().try_car().unwrap();
                let nested = names.try_attrib().unwrap().try_car().unwrap();
                assert!(
                    nested
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq("é".as_bytes())
                        .unwrap()
                );
                assert!(
                    nested
                        .try_string_elt(1)
                        .unwrap()
                        .try_char_eq("日本".as_bytes())
                        .unwrap()
                );
                assert!(nested.try_string_elt(2).unwrap().try_char_eq(b"").unwrap());
            }
        });
    }
}

#[test]
fn padded_names_metadata_change_during_publication_does_not_publish() {
    use crate::sexp::{ffi::NodeBody, heap::ResolvedLink, object::SexpMut};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let token = session.owner_token().unwrap();
        let factory = token.node_factory();
        let target = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 3)))
            .unwrap()
            .into_owned()
            .unwrap();
        let input = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
            .unwrap()
            .into_owned()
            .unwrap();
        let labels = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 2)))
            .unwrap()
            .into_owned()
            .unwrap();
        let name = unsafe { token.sexp(super::R_NamesSymbol()) }
            .unwrap()
            .into_owned()
            .unwrap();
        unsafe { super::setAttrib(input.as_raw(), name.as_raw(), labels.as_raw()) };
        let heap = target.allocation().unwrap().heap_identity();
        let name_link = name.link_in(&heap).unwrap();
        let changed = Rc::new(Cell::new(false));
        let callback_changed = changed.clone();
        let callback_input = input.clone();
        let nil = factory.nil().into_owned().unwrap();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if callback_changed.get() {
                return;
            }
            let final_cell_exists =
                crate::sexp::instance::with_required_current_instance(|runtime| unsafe {
                    (*runtime).arena.active_nodes().any(|pointer| {
                        let Some(node) = (*runtime).arena.node_token(pointer) else {
                            return false;
                        };
                        let Some(header) = node.heap_identity().node_snapshot(&node) else {
                            return false;
                        };
                        let NodeBody::List(cell) = header.data else {
                            return false;
                        };
                        if header.sxpinfo.type_of() != SEXPTYPE::LISTSXP || cell.tagval != name_link
                        {
                            return false;
                        }
                        let Some(ResolvedLink::Node { allocation, .. }) =
                            heap.resolve_link(cell.carval)
                        else {
                            return false;
                        };
                        let Some(value) = heap.node_snapshot(&allocation) else {
                            return false;
                        };
                        value.sxpinfo.type_of() == SEXPTYPE::STRSXP && !value.attrib.is_null()
                    })
                });
            if final_cell_exists {
                callback_changed.set(true);
                SexpMut::try_from_checked(callback_input.clone())
                    .unwrap()
                    .try_set_attribute(&nil)
                    .unwrap();
            }
        }));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            super::setAttrib(target.as_raw(), name.as_raw(), input.as_raw());
        }));
        unsafe { crate::mainutils::memory_main::R_gc_torture(0, 0, 0) };
        assert!(
            changed.get(),
            "source metadata changed during final publication allocation"
        );
        assert!(result.unwrap_err().is::<crate::sexp::context::RError>());
        assert!(target.try_attrib().unwrap().is_nil());
    });
}
