use super::*;
use crate::sexp::{accessors::*, memory::ArenaBudget};
use std::cell::Cell;

struct Numbers {
    state: Rc<Cell<u8>>,
    collect: bool,
}
impl AltrepClass for Numbers {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(c.data2().try_integer_elt(0)? as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        if self.collect {
            c.gc()?;
        }
        match self.state.get() {
            1 if i == 2 => return Err(failure("injected element failure")),
            2 if i == 2 => panic!("injected callback unwind"),
            3 => {
                force_materialization(&c.object())?;
            }
            4 => {
                c.object().try_real_elt(i)?;
            }
            5 => return Ok(AltrepElement::Integer(7)),
            _ => (),
        }
        Ok(AltrepElement::Real(
            c.data1().try_real_elt(0)? + i as f64 * 0.5,
        ))
    }
}
fn vector<'s>(s: &'s RSession, kind: SEXPTYPE, n: i64) -> Sexp<'s> {
    allocate(s.owner_token().unwrap(), kind, n).unwrap()
}
fn integer<'s>(s: &'s RSession, value: i32) -> Sexp<'s> {
    let mut v = SexpMut::try_from_checked(vector(s, SEXPTYPE::INTSXP, 1)).unwrap();
    v.try_set_integer_elt(0, value).unwrap();
    v.freeze()
}
fn real<'s>(s: &'s RSession, value: f64) -> Sexp<'s> {
    let mut v = SexpMut::try_from_checked(vector(s, SEXPTYPE::REALSXP, 1)).unwrap();
    v.try_set_real_elt(0, value).unwrap();
    v.freeze()
}
fn numbers<'s>(s: &'s RSession, state: Rc<Cell<u8>>, collect: bool) -> Sexp<'s> {
    let class = s
        .register_altrep_class("numbers", Numbers { state, collect })
        .unwrap();
    AltrepBuilder::new(class)
        .data1(real(s, 10.0))
        .data2(integer(s, 5))
        .build()
        .unwrap()
}
#[test]
fn lazy_callbacks_and_materialization_survive_collection() {
    let s = RSession::new_for_gc_tests();
    let x = numbers(&s, Rc::new(Cell::new(0)), true);
    assert_eq!(x.len(), 5);
    for i in 0..5 {
        assert_eq!(x.try_real_elt(i).unwrap(), 10.0 + i as f64 * 0.5);
    }
    assert!(!is_materialized(&x));
    assert!(matches!(
        x.try_real_elt(-1),
        Err(SexpError::OutOfBounds { .. })
    ));
    assert!(matches!(
        x.try_real_elt(5),
        Err(SexpError::OutOfBounds { .. })
    ));
    force_materialization(&x).unwrap();
    assert!(is_materialized(&x));
    s.gc();
    let mut mut_x = SexpMut::try_from_checked(x.clone()).unwrap();
    mut_x.try_set_real_elt(2, 77.0).unwrap();
    assert_eq!(x.real_elt(2), Some(77.0));
    assert_eq!(unsafe { *REAL(x.clone().as_raw()).add(2) }, 77.0);
    assert!(altrep_class(&x).is_some());
    let duplicate =
        s.with_active(|| unsafe { crate::mainutils::duplicate::Rf_duplicate(x.clone().as_raw()) });
    let duplicate = s.sexp(duplicate).unwrap();
    assert_eq!(duplicate.real_elt(2), Some(77.0));
    assert!(!is_altrep(&duplicate));
}
#[test]
fn partial_failure_and_unwind_are_retryable() {
    for mode in [1, 2, 3, 4, 5] {
        let s = RSession::new_for_gc_tests();
        let state = Rc::new(Cell::new(mode));
        let x = numbers(&s, state.clone(), true);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| force_materialization(&x)));
        if mode == 2 {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_err());
        }
        assert!(!is_materialized(&x));
        assert!(x.header().payload.is_null());
        state.set(0);
        s.gc();
        force_materialization(&x).unwrap();
        assert_eq!(x.real_elt(4), Some(12.0));
    }
}
#[test]
fn repeat_every_vector_type_retains_traced_children_and_mutates() {
    let s = RSession::new_for_gc_tests();
    for kind in [
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::LGLSXP,
        SEXPTYPE::RAWSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::STRSXP,
        SEXPTYPE::VECSXP,
    ] {
        let value = vector(&s, kind, 1);
        let mut value = SexpMut::try_from_checked(value).unwrap();
        let child = real(&s, 19.0);
        match kind {
            SEXPTYPE::INTSXP => value.try_set_integer_elt(0, 42).unwrap(),
            SEXPTYPE::REALSXP => value.try_set_real_elt(0, 42.5).unwrap(),
            SEXPTYPE::LGLSXP => value.try_set_logical_elt(0, 1).unwrap(),
            SEXPTYPE::RAWSXP => value.try_set_raw_elt(0, 255).unwrap(),
            SEXPTYPE::CPLXSXP => value
                .try_set_complex_elt(0, Rcomplex { r: 4.0, i: 2.0 })
                .unwrap(),
            SEXPTYPE::STRSXP => {
                let raw = s.with_active(|| unsafe {
                    super::super::memory::with_arena(|a| a.alloc_charsxp(b"hello"))
                });
                value.try_set_string_elt(0, s.sexp(raw).unwrap()).unwrap();
            }
            SEXPTYPE::VECSXP => value.try_set_vector_elt(0, child).unwrap(),
            _ => unreachable!(),
        }
        let class = s
            .register_altrep_class(&format!("repeat-{}", kind.0), RepeatClass(kind))
            .unwrap();
        let x = AltrepBuilder::new(class)
            .data1(value.freeze())
            .data2(real(&s, 4.0))
            .build()
            .unwrap();
        s.gc();
        let elt = altrep_elt(&x, 3).unwrap();
        match elt {
            AltrepElement::Integer(v) => assert_eq!(v, 42),
            AltrepElement::Real(v) => assert_eq!(v, 42.5),
            AltrepElement::Logical(v) => assert_eq!(v, 1),
            AltrepElement::Raw(v) => assert_eq!(v, 255),
            AltrepElement::Complex(v) => assert_eq!((v.r, v.i), (4.0, 2.0)),
            AltrepElement::String(v) => assert_eq!(v.as_string().as_deref(), Some("hello")),
            AltrepElement::List(v) => assert_eq!(v.real_elt(0), Some(19.0)),
        }
        force_materialization(&x).unwrap();
        s.gc();
        assert!(altrep_elt(&x, 3).is_ok());
    }
}
#[test]
fn original_owner_budget_and_cross_session_data_are_checked() {
    let s = RSession::new_for_gc_tests();
    let x = numbers(&s, Rc::new(Cell::new(0)), true);
    let other = RSession::new_for_gc_tests();
    // Tighten only the original owner while retaining rooted handles.
    let token = s.owner_token().unwrap();
    let original = s.arena_budget();
    unsafe {
        (*token.as_ptr()).arena.set_budget(ArenaBudget {
            max_bytes: 1,
            ..original
        });
    }
    assert!(force_materialization(&x).is_err());
    assert_eq!(x.real_elt(4), Some(12.0));
    assert_eq!(
        crate::sexp::instance::current_instance_ptr(),
        Some(other.owner_token().unwrap().as_ptr())
    );
    unsafe {
        (*token.as_ptr()).arena.set_budget(original);
    }
    force_materialization(&x).unwrap();
    let class = s
        .register_altrep_class("foreign", RepeatClass(SEXPTYPE::REALSXP))
        .unwrap();
    let child = real(&other, 3.0);
    assert!(matches!(
        AltrepBuilder::new(class)
            .data1(child)
            .data2(real(&s, 2.0))
            .build(),
        Err(SexpError::UnownedPointer { .. })
    ));
}
#[test]
fn compact_vectors_work_with_feature_and_do_not_recurse_on_duplicate() {
    let s = RSession::new_for_gc_tests();
    s.with_active(|| unsafe {
        let raw = crate::mainutils::altrep::R_compact_intseq(5, 1);
        let x = s.sexp(raw).unwrap();
        assert_eq!(crate::mainutils::altclasses::R_compact_intseq_check(raw), 1);
        assert_eq!(
            crate::mainutils::altrep::ALTINTEGER_ELT(raw, -1),
            crate::sexp::ffi::NA_INTEGER
        );
        assert_eq!(crate::mainutils::altrep::ALTINTEGER_ELT(raw, 4), 1);
        s.gc();
        let dup = crate::mainutils::duplicate::Rf_duplicate(raw);
        assert_eq!(s.sexp(dup).unwrap().integer_elt(4), Some(1));
        assert_eq!(x.integer_elt(4), Some(1));
        let empty = crate::mainutils::altrep::R_compact_realseq(1.0, 1.0, 0);
        assert_eq!(XLENGTH(empty), 0);
    });
}

struct FreshStrings;
impl AltrepClass for FreshStrings {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(3)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        c.gc()?;
        let text = c.string(&format!("value-{i}"))?;
        c.gc()?;
        Ok(AltrepElement::String(text))
    }
}
#[test]
fn fresh_string_children_remain_rooted_and_borrowed_text_is_traced() {
    let s = RSession::new_for_gc_tests();
    let cls = s
        .register_altrep_class("fresh-strings", FreshStrings)
        .unwrap();
    let x = AltrepBuilder::new(cls).build().unwrap();
    let child = x.try_string_elt(2).unwrap();
    assert!(!is_materialized(&x));
    s.gc();
    assert_eq!(child.as_string().as_deref(), Some("value-2"));
    drop(child);
    force_materialization(&x).unwrap();
    assert!(is_materialized(&x));
    s.gc();
    assert_eq!(
        x.try_string_value_elt(2).unwrap().as_deref(),
        Some("value-2")
    );
    let mut guard = SexpMut::try_from_checked(x.clone()).unwrap();
    let raw = s
        .with_active(|| unsafe { crate::sexp::memory::with_arena(|a| a.alloc_charsxp(b"edited")) });
    guard.try_set_string_elt(1, s.sexp(raw).unwrap()).unwrap();
    s.gc();
    assert_eq!(
        x.try_string_value_elt(1).unwrap().as_deref(),
        Some("edited")
    );
}
#[test]
fn serialize_falls_back_to_plain_values_without_internal_metadata() {
    let s = RSession::new_for_gc_tests();
    let x = numbers(&s, Rc::new(Cell::new(0)), true);
    s.with_active(|| unsafe {
        use crate::mainutils::serialize::{R_serialize, R_unserialize};
        let nil = crate::sexp::globals::R_NilValue();
        let bytes = R_serialize(x.clone().as_raw(), nil, nil, nil, nil);
        let bytes = s.sexp(bytes).unwrap();
        s.gc();
        let copy = R_unserialize(bytes.clone().as_raw(), nil);
        let copy = s.sexp(copy).unwrap();
        assert_eq!(copy.len(), 5);
        assert_eq!(copy.real_elt(4), Some(12.0));
        assert!(!is_altrep(&copy));
        assert!(
            copy.attrib()
                .is_none_or(|v| v.typeof_() == SEXPTYPE::NILSXP)
        );
        assert!(!is_materialized(&x));
    });
}
#[test]
fn native_class_metadata_and_reentrant_registration_work() {
    let s = RSession::new_for_gc_tests();
    s.with_active(|| unsafe {
        use crate::mainutils::altrep::*;
        unsafe extern "C" fn length(x: SEXP) -> i64 {
            unsafe { INTEGER_ELT(R_altrep_data2(x), 0) as i64 }
        }
        unsafe extern "C" fn element(x: SEXP, i: i64) -> f64 {
            unsafe {
                // A callback is allowed to update its class table; no RefCell
                // or RInstance borrow may survive invocation.
                R_set_altreal_Elt_method(R_altrep_class(x), Some(element));
                crate::sexp::gengc::full_gc();
                REAL_ELT(R_altrep_data1(x), 0) + i as f64
            }
        }
        let cls = R_make_altreal_class(c"native".as_ptr(), c"test".as_ptr(), std::ptr::null_mut());
        R_set_altrep_length_method(cls, Some(length));
        R_set_altreal_Elt_method(cls, Some(element));
        let d1 = real(&s, 20.0);
        let d2 = integer(&s, 4);
        let x = R_new_altrep(cls, d1.clone().as_raw(), d2.clone().as_raw());
        let x = s.sexp(x).unwrap();
        drop(d1);
        drop(d2);
        s.gc();
        assert_eq!(REAL_ELT(x.clone().as_raw(), 3), 23.0);
        assert!(!is_materialized(&x));
        force_materialization(&x).unwrap();
        assert_eq!(*REAL(x.clone().as_raw()).add(3), 23.0);
        let d1 = real(&s, 100.0);
        R_set_altrep_data1(x.clone().as_raw(), d1.clone().as_raw());
        drop(d1);
        s.gc();
        assert_eq!(REAL_ELT(R_altrep_data1(x.clone().as_raw()), 0), 100.0);
        assert_eq!(x.real_elt(3), Some(23.0));
    });
}
#[test]
fn zero_negative_and_invalid_scalar_lengths_are_handled() {
    let s = RSession::new_for_gc_tests();
    let cls = s
        .register_altrep_class("repeat", RepeatClass(SEXPTYPE::REALSXP))
        .unwrap();
    for length in [-1.0, f64::NAN, f64::INFINITY, 0.5, i64::MAX as f64] {
        assert!(
            AltrepBuilder::new(cls.clone())
                .data1(real(&s, 1.0))
                .data2(real(&s, length))
                .build()
                .is_err()
        );
    }
    let empty = AltrepBuilder::new(cls)
        .data1(real(&s, 1.0))
        .data2(real(&s, 0.0))
        .build()
        .unwrap();
    assert!(empty.is_empty());
    force_materialization(&empty).unwrap();
    assert!(altrep_elt(&empty, 0).is_err());
}

#[test]
fn expanded_cache_outlives_original_and_releases_final_buffer_lease() {
    let s = RSession::new_for_gc_tests();
    let token = s.owner_token().unwrap();
    let before;
    let cache;
    let payload;
    {
        let raw =
            s.with_active(|| unsafe { crate::mainutils::altrep::R_compact_realseq(4.0, 0.5, 8) });
        let x = s.sexp(raw).unwrap();
        before = unsafe { (*token.as_ptr()).arena.total_bytes_allocated() };
        force_materialization(&x).unwrap();
        cache = s.with_active(|| unsafe {
            s.sexp(crate::mainutils::altrep::R_altrep_data2(raw))
                .unwrap()
        });
        assert_eq!(cache.len(), 8);
        assert_eq!(cache.header().payload, x.header().payload);
        payload = x.header().payload;
        let mut cache_mut = SexpMut::try_from_checked(cache.clone()).unwrap();
        cache_mut.try_set_real_elt(1, 99.0).unwrap();
        assert_eq!(x.real_elt(1), Some(99.0));
        let after = unsafe { (*token.as_ptr()).arena.total_bytes_allocated() };
        assert_eq!(
            after - before,
            std::mem::size_of::<crate::sexp::ffi::SexprecCore>() + 8 * std::mem::size_of::<f64>()
        );
    }
    s.with_active(|| token.full_gc().unwrap());
    assert_eq!(cache.real_elt(1), Some(99.0));
    assert_eq!(cache.real_elt(7), Some(7.5));
    // The original header is gone; the independently rooted cache is sole owner.
    drop(cache);
    s.with_active(|| token.full_gc().unwrap());
    assert!(!s.with_active(|| unsafe {
        crate::sexp::memory::with_arena(|a| a.tracks_altrep_test_buffer(payload as *mut u8))
    }));
}
#[test]
fn replacing_attributes_keeps_payload_owned_after_cache_is_swept() {
    let s = RSession::new_for_gc_tests();
    let x = numbers(&s, Rc::new(Cell::new(0)), true);
    s.with_active(|| unsafe { SET_ATTRIB(x.clone().as_raw(), crate::sexp::globals::R_NilValue()) });
    assert!(!is_altrep(&x));
    s.gc();
    assert_eq!(x.real_elt(4), Some(12.0));
    let mut x_mut = SexpMut::try_from_checked(x.clone()).unwrap();
    x_mut.try_set_real_elt(0, 5.0).unwrap();
    assert_eq!(x.real_elt(0), Some(5.0));
}
#[test]
fn deferred_evaluation_caches_a_rooted_result() {
    let s = RSession::new_for_gc_tests();
    let result = real(&s, 32.0);
    let data = vector(&s, SEXPTYPE::VECSXP, 3);
    let mut data = SexpMut::try_from_checked(data).unwrap();
    data.try_set_vector_elt(0, result).unwrap();
    data.try_set_vector_elt(1, s.global_env().unwrap()).unwrap();
    data.try_set_vector_elt(2, real(&s, 1.0)).unwrap();
    let class = s
        .register_altrep_class("deferred", DeferredClass(SEXPTYPE::REALSXP))
        .unwrap();
    let x = AltrepBuilder::new(class)
        .data1(data.freeze())
        .build()
        .unwrap();
    assert_eq!(x.real_elt(0), Some(32.0));
    let cache = metadata(&x).unwrap().vector_elt(2).unwrap();
    assert_eq!(cache.real_elt(0), Some(32.0));
    s.gc();
    assert_eq!(x.real_elt(0), Some(32.0));
    force_materialization(&x).unwrap();
    s.gc();
    assert_eq!(x.real_elt(0), Some(32.0));
}

#[cfg(target_pointer_width = "32")]
#[test]
fn long_integer_sequence_rejects_unrepresentable_expansion_without_truncation() {
    let s = RSession::new_for_gc_tests();
    let raw = s.with_active(|| unsafe {
        crate::mainutils::altrep::R_compact_intseq(i32::MIN as i64, i32::MAX as i64)
    });
    let x = s.sexp(raw).unwrap();
    assert_eq!(x.len(), 1_i64 << 32);
    assert_eq!(x.integer_elt((1_i64 << 32) - 1), Some(i32::MAX));
    assert!(force_materialization(&x).is_err());
    assert!(x.header().payload.is_null());
    assert_eq!(x.integer_elt((1_i64 << 32) - 1), Some(i32::MAX));
}

#[test]
fn native_pointer_elements_are_retained_without_bulk_expansion() {
    let s = RSession::new_for_gc_tests();
    let class = s
        .register_altrep_class("native-fresh", FreshStrings)
        .unwrap();
    let x = AltrepBuilder::new(class).build().unwrap();
    let raw = s.with_active(|| unsafe { STRING_ELT(x.clone().as_raw(), 1) });
    s.with_active(|| s.owner_token().unwrap().full_gc().unwrap());
    // The raw child has no independent lease. Its parent sparse cache roots it.
    let child = s
        .sexp(raw)
        .expect("native result must remain retained by parent");
    assert_eq!(child.as_string().as_deref(), Some("value-1"));
    assert!(!is_materialized(&x));
    let cache_head = metadata(&x).unwrap().vector_elt(4).unwrap();
    let again = s.with_active(|| unsafe { STRING_ELT(x.clone().as_raw(), 1) });
    assert_eq!(
        metadata(&x).unwrap().vector_elt(4).unwrap().as_raw(),
        cache_head.as_raw()
    );
    assert_eq!(
        s.sexp(again).unwrap().as_string().as_deref(),
        Some("value-1")
    );
}

#[test]
fn raw_dataptr_expands_extension_without_discarding_class_or_data() {
    let s = RSession::new_for_gc_tests();
    let x = numbers(&s, Rc::new(Cell::new(0)), true);
    let descriptor = altrep_class(&x).unwrap();
    let ptr = s.with_active(|| unsafe { REAL(x.clone().as_raw()) });
    assert!(!ptr.is_null());
    assert_eq!(unsafe { *ptr.add(4) }, 12.0);
    assert!(is_altrep(&x));
    assert_eq!(altrep_class(&x).unwrap().as_raw(), descriptor.as_raw());
    let source =
        s.with_active(|| unsafe { crate::mainutils::altrep::R_altrep_data1(x.clone().as_raw()) });
    assert_eq!(s.sexp(source).unwrap().real_elt(0), Some(10.0));
    s.with_active(|| s.owner_token().unwrap().full_gc().unwrap());
    assert_eq!(x.real_elt(4), Some(12.0));
}

#[test]
fn native_scalar_callback_failure_raises_error_instead_of_missing_value() {
    let s = RSession::new_for_gc_tests();
    let state = Rc::new(Cell::new(1));
    let x = numbers(&s, state.clone(), false);
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.with_active(|| unsafe { crate::mainutils::altrep::ALTREAL_ELT(x.clone().as_raw(), 2) })
    }))
    .unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .is_some()
    );
    state.set(0);
    assert_eq!(
        s.with_active(|| unsafe { crate::mainutils::altrep::ALTREAL_ELT(x.clone().as_raw(), 2) }),
        11.0
    );
}

#[test]
fn raw_dispatch_and_classification_respect_existing_arena_lends() {
    let s = RSession::new_for_gc_tests();
    let dense = integer(&s, 6);
    let formula = s.compact_integer_sequence(3, 1, 4).unwrap();
    let extension = s.with_active(|| unsafe {
        s.sexp(crate::mainutils::altrep::R_compact_intseq(1, 4))
            .unwrap()
    });
    s.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let count = arena.node_count();
            assert_eq!(INTEGER_ELT(dense.clone().as_raw(), 0), 6);
            assert_eq!(INTEGER_ELT(formula.clone().as_raw(), 3), 6);
            assert_eq!(
                crate::mainutils::altclasses::R_compact_intseq_check(dense.clone().as_raw()),
                0
            );
            assert_eq!(
                crate::mainutils::altclasses::R_compact_intseq_check(formula.clone().as_raw()),
                1
            );
            assert_eq!(
                crate::mainutils::altclasses::R_compact_intseq_check(extension.clone().as_raw()),
                1
            );
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                DATAPTR(extension.clone().as_raw())
            }))
            .unwrap_err();
            assert!(
                failure
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some()
            );
            assert_eq!(arena.node_count(), count);
        });
    });
    force_materialization(&extension).unwrap();
    assert_eq!(extension.integer_elt(3), Some(4));
}
struct SelfList;
impl AltrepClass for SelfList {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::VECSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        Ok(AltrepElement::List(c.object()))
    }
}
#[test]
fn serialization_cycle_fails_cleanly_and_resets_rust_operation_guard() {
    let s = RSession::new_for_gc_tests();
    let class = s.register_altrep_class("self-list", SelfList).unwrap();
    let x = AltrepBuilder::new(class).build().unwrap();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.with_active(|| unsafe {
            let nil = crate::sexp::globals::R_NilValue();
            crate::mainutils::serialize::R_serialize(x.clone().as_raw(), nil, nil, nil, nil)
        })
    }))
    .unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .is_some()
    );
    assert!(unsafe {
        (*s.owner_token().unwrap().as_ptr())
            .altrep_state
            .active
            .borrow()
            .is_empty()
    });
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.with_active(|| unsafe { crate::mainutils::duplicate::Rf_duplicate(x.clone().as_raw()) })
    }))
    .unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .is_some()
    );
    assert!(unsafe {
        (*s.owner_token().unwrap().as_ptr())
            .altrep_state
            .active
            .borrow()
            .is_empty()
    });
    force_materialization(&x).unwrap();
    s.gc();
    assert_eq!(x.vector_elt(0).unwrap().as_raw(), x.clone().as_raw());
}

#[test]
fn native_duplicate_inspect_and_coerce_restore_original_owner() {
    let s = RSession::new_for_gc_tests();
    s.with_active(|| unsafe {
        use crate::mainutils::altrep::*;
        unsafe extern "C" fn length(_: SEXP) -> i64 {
            1
        }
        unsafe extern "C" fn duplicate(x: SEXP, _: i32) -> SEXP {
            let result = unsafe { R_altrep_data1(x) };
            let other = RSession::new_for_gc_tests();
            other.gc();
            drop(other);
            result
        }
        unsafe extern "C" fn inspect(_: SEXP, _: i32, _: i32) -> i32 {
            let other = RSession::new_for_gc_tests();
            drop(other);
            1
        }
        unsafe extern "C" fn coerce(x: SEXP, _: i32) -> SEXP {
            unsafe { duplicate(x, 0) }
        }
        let class =
            R_make_altreal_class(c"restore".as_ptr(), c"test".as_ptr(), std::ptr::null_mut());
        R_set_altrep_length_method(class, Some(length));
        R_set_altrep_duplicate_method(class, Some(duplicate));
        R_set_altrep_inspect_method(class, Some(inspect));
        R_set_altrep_coerce_method(class, Some(coerce));
        let data = real(&s, 50.0);
        let x = s
            .sexp(R_new_altrep(
                class,
                data.clone().as_raw(),
                crate::sexp::globals::R_NilValue(),
            ))
            .unwrap();
        let expected_owner = s.owner_token().unwrap().as_ptr();
        let dup = crate::mainutils::duplicate::Rf_duplicate(x.clone().as_raw());
        assert_eq!(s.sexp(dup).unwrap().real_elt(0), Some(50.0));
        assert_eq!(
            crate::sexp::instance::current_instance_ptr(),
            Some(expected_owner)
        );
        assert_eq!(R_altrep_inspect(x.clone().as_raw(), 0, 0), 1);
        assert_eq!(
            crate::sexp::instance::current_instance_ptr(),
            Some(expected_owner)
        );
        let coerced = R_altrep_coerce(x.clone().as_raw(), SEXPTYPE::REALSXP.0 as i32);
        assert_eq!(s.sexp(coerced).unwrap().real_elt(0), Some(50.0));
        assert_eq!(
            crate::sexp::instance::current_instance_ptr(),
            Some(expected_owner)
        );
    });
}
