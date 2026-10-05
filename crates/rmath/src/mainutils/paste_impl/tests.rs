//! Actual native paste entry with independently checked GNU UTF8 results.
use crate::sexp::{
    RSession,
    ffi::SEXPTYPE,
    object::{SessionNodeFactory, Sexp},
};

fn fixture(
    f: &SessionNodeFactory<'_>,
    separator: &str,
    collapse: Option<&str>,
    extra: bool,
) -> Sexp<'static> {
    let input = f.strings(&["é", "", "z"]).unwrap();
    let missing = f
        .wrap(unsafe { crate::sexp::globals::R_NaString() })
        .unwrap();
    unsafe {
        crate::sexp::accessors::SET_STRING_ELT(input.as_raw(), 1, missing.as_raw());
    }
    let second = f.strings(&["β"]).unwrap();
    let inputs = f
        .allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::VECSXP, if extra { 2 } else { 1 });
            unsafe {
                crate::sexp::accessors::SET_VECTOR_ELT(pointer, 0, input.as_raw());
                if extra {
                    crate::sexp::accessors::SET_VECTOR_ELT(pointer, 1, second.as_raw());
                }
            }
            Some(pointer)
        })
        .unwrap();
    let separator = f.strings(&[separator]).unwrap();
    let collapse = collapse.map_or_else(|| f.nil(), |s| f.strings(&[s]).unwrap());
    let recycle = f.domain().logical(false);
    let mut args = f.nil();
    for value in [&recycle, &collapse, &separator, &inputs] {
        args = f.pairlist_cell(value, &args, &f.nil()).unwrap();
    }
    args.into_owned().unwrap()
}
fn invoke(f: &SessionNodeFactory<'_>, args: &Sexp<'_>) -> Sexp<'static> {
    let op = f
        .wrap(unsafe {
            crate::eval::primitive::make_primitive_binding("paste", SEXPTYPE::BUILTINSXP)
        })
        .unwrap();
    f.wrap(unsafe {
        super::do_paste(
            f.nil().as_raw(),
            op.as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
    .into_owned()
    .unwrap()
}
fn texts(value: &Sexp<'_>) -> Vec<String> {
    (0..value.len())
        .map(|i| value.try_string_value_elt(i).unwrap().unwrap())
        .collect()
}
#[test]
fn owned_paste_utf8_empty_separator_preserves_missing_text() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let args = fixture(&f, "", None, false);
        assert_eq!(texts(&invoke(&f, &args)), ["é", "NA", "z"]);
    });
}
#[test]
fn owned_paste_utf8_selected_separator_and_collapse_match_gnu() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let args = fixture(&f, "é", Some("|"), true);
        assert_eq!(texts(&invoke(&f, &args)), ["ééβ|NAéβ|zéβ"]);
    });
}

fn invoke_op(f: &SessionNodeFactory<'_>, args: &Sexp<'_>, op: &Sexp<'_>) -> Sexp<'static> {
    f.wrap(unsafe {
        super::do_paste(
            f.nil().as_raw(),
            op.as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
    .into_owned()
    .unwrap()
}
fn collecting_allocation(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let selection = Rc::new(Cell::new(0));
    let class = facade
        .borrow()
        .as_ref()
        .unwrap()
        .register_altrep_class(
            "paste.separator.ownership",
            CollapseSelection(selection.clone()),
        )
        .unwrap()
        .into_owned()
        .unwrap();
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let f = weak.node_factory().unwrap();
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let args = fixture(&f, "é", None, true);
            let sep_cell = args.try_cdr().unwrap();
            let separator = sep_cell.try_car().unwrap().into_owned().unwrap();
            let sep = separator
                .try_string_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let provider = crate::sexp::altrep::AltrepBuilder::new(class)
                .data1(separator.clone())
                .build()
                .unwrap();
            crate::sexp::accessors::SETCAR(sep_cell.as_raw(), provider.as_raw());
            drop(provider);
            drop(sep_cell);
            let x = args.try_car().unwrap().allocation().unwrap().clone();
            let op = f
                .wrap(crate::eval::primitive::make_primitive_binding(
                    "paste",
                    SEXPTYPE::BUILTINSXP,
                ))
                .unwrap();
            let called = Rc::new(Cell::new(0));
            let count = called.clone();
            let original = weak.clone();
            let saved = args.clone();
            let revoker = Rc::downgrade(&facade);
            let selected = sep.clone();
            let source = x.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if selection.get() < 2 || count.get() != 0 {
                    return;
                }
                count.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                let f = original.node_factory().unwrap();
                crate::sexp::accessors::SET_STRING_ELT(
                    separator.as_raw(),
                    0,
                    crate::sexp::globals::R_NaString(),
                );
                crate::sexp::accessors::SETCAR(saved.as_raw(), f.nil().as_raw());
                crate::sexp::gengc::full_gc();
                assert!(
                    selected.is_live(),
                    "the selected separator owns its detached scalar"
                );
                assert!(
                    source.is_live(),
                    "the actual input vector remains owned after argument detachment"
                );
                match action {
                    1 => std::panic::panic_any(967_u32),
                    2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                    _ => {}
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = catch_unwind(AssertUnwindSafe(|| invoke_op(&f, &args, &op)));
            assert_eq!(called.get(), 1);
            assert_eq!((*instance).memory_state.in_gc, 0);
            match action {
                0 => {
                    let value = result.unwrap();
                    assert_eq!(texts(&value), ["ééβ", "NAéβ", "zéβ"]);
                    crate::sexp::gengc::full_gc();
                    assert!(!sep.is_live());
                    assert!(!x.is_live());
                    assert_eq!(texts(&value), ["ééβ", "NAéβ", "zéβ"]);
                }
                1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 967),
                2 => {
                    assert!(!weak.is_live());
                    assert!(
                        result
                            .unwrap_err()
                            .downcast_ref::<crate::sexp::context::RError>()
                            .is_some()
                    );
                }
                _ => unreachable!(),
            }
        });
    }
}
#[test]
fn owned_paste_selected_separator_and_source_survive_detachment_collection_then_release() {
    collecting_allocation(0);
}
#[test]
fn owned_paste_live_callback_panic_preserves_original_payload() {
    collecting_allocation(1);
}
#[test]
fn owned_paste_revoked_original_cannot_publish() {
    collecting_allocation(2);
}

struct MutateLater(std::rc::Rc<std::cell::Cell<usize>>);
impl crate::sexp::altrep::AltrepClass for MutateLater {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(
        &self,
        _: &crate::sexp::altrep::AltrepContext<'_>,
    ) -> crate::sexp::object::SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        c: &crate::sexp::altrep::AltrepContext<'s>,
        _: i64,
    ) -> crate::sexp::object::SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
        let value = c.data1()?;
        let selected = value.try_string_elt(0)?;
        if self.0.get() == 0 {
            crate::sexp::object::SexpMut::try_from_checked(c.data2()?)?
                .try_set_vector_elt(1, value)?;
            unsafe {
                crate::sexp::gengc::full_gc();
            }
        }
        self.0.set(self.0.get() + 1);
        Ok(crate::sexp::altrep::AltrepElement::String(selected))
    }
}
#[test]
fn owned_paste_provider_mutation_observes_later_elements_in_gnu_pass_order() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    let count = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("paste.pass.order", MutateLater(count.clone()))
        .unwrap();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let args = fixture(&f, "|", None, true);
        let x = args.try_car().unwrap();
        let source = f.strings(&["é"]).unwrap();
        let provider = crate::sexp::altrep::AltrepBuilder::new(class)
            .data1(source)
            .data2(x.clone())
            .build()
            .unwrap();
        crate::sexp::object::SexpMut::try_from_checked(x)
            .unwrap()
            .try_set_vector_elt(0, provider)
            .unwrap();
        assert_eq!(texts(&invoke(&f, &args)), ["é|é"]);
        assert_eq!(
            count.get(),
            3,
            "flag, width and copy passes retain their actual callback ordering"
        );
    });
}

struct CollapseSelection(std::rc::Rc<std::cell::Cell<usize>>);
impl crate::sexp::altrep::AltrepClass for CollapseSelection {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(
        &self,
        _: &crate::sexp::altrep::AltrepContext<'_>,
    ) -> crate::sexp::object::SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        c: &crate::sexp::altrep::AltrepContext<'s>,
        _: i64,
    ) -> crate::sexp::object::SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
        self.0.set(self.0.get() + 1);
        Ok(crate::sexp::altrep::AltrepElement::String(
            c.data1()?.try_string_elt(0)?,
        ))
    }
}
#[test]
fn owned_paste_selected_collapse_survives_detachment_during_output_allocation() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    let selected = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "paste.collapse.ownership",
            CollapseSelection(selected.clone()),
        )
        .unwrap();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let pin = owner.pin().unwrap().expect("managed test session");
        let instance = pin.as_ptr();
        let f = owner.node_factory();
        let args = fixture(&f, "é", None, true);
        let collapse_cell = args.try_cdr().unwrap().try_cdr().unwrap();
        let source = f.strings(&["|"]).unwrap().into_owned().unwrap();
        let scalar = source
            .try_string_elt(0)
            .unwrap()
            .allocation()
            .unwrap()
            .clone();
        let provider = crate::sexp::altrep::AltrepBuilder::new(class)
            .data1(source.clone())
            .build()
            .unwrap();
        unsafe {
            crate::sexp::accessors::SETCAR(collapse_cell.as_raw(), provider.as_raw());
        }
        let op = f
            .wrap(unsafe {
                crate::eval::primitive::make_primitive_binding("paste", SEXPTYPE::BUILTINSXP)
            })
            .unwrap();
        let ran = Rc::new(Cell::new(false));
        let called = ran.clone();
        let selection = selected.clone();
        let witness = scalar.clone();
        unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if selection.get() < 2 || called.replace(true) {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                crate::sexp::accessors::SET_STRING_ELT(
                    source.as_raw(),
                    0,
                    crate::sexp::globals::R_NaString(),
                );
                crate::sexp::gengc::full_gc();
                assert!(witness.is_live());
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        }
        let output = invoke_op(&f, &args, &op);
        assert!(ran.get());
        assert_eq!(selected.get(), 2);
        assert_eq!(texts(&output), ["ééβ|NAéβ|zéβ"]);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert!(!scalar.is_live());
        assert_eq!(texts(&output), ["ééβ|NAéβ|zéβ"]);
    });
}
#[test]
fn owned_paste_zero_length_and_invalid_separator_keep_gnu_admission() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let args = fixture(&f, "", None, false);
        let input = args.try_car().unwrap();
        let empty = f.strings(&[]).unwrap();
        crate::sexp::object::SexpMut::try_from_checked(input)
            .unwrap()
            .try_set_vector_elt(0, empty)
            .unwrap();
        assert!(invoke(&f, &args).is_empty());
        let sep = args.try_cdr().unwrap().try_car().unwrap();
        unsafe {
            crate::sexp::accessors::SET_STRING_ELT(
                sep.as_raw(),
                0,
                crate::sexp::globals::R_NaString(),
            );
        }
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &args)))
            .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "invalid separator"
        );
        let good = fixture(&f, "", None, false);
        assert_eq!(texts(&invoke(&f, &good)), ["é", "NA", "z"]);
    });
}
