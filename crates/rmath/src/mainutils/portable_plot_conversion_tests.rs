use super::native_convert;
use crate::sexp::{
    RSession,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::SEXPTYPE,
    object::SexpResult,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Provider {
    calls: Rc<Cell<usize>>,
    revoke: bool,
    change_layout: bool,
    instance: *mut crate::sexp::instance::RInstance,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: i64,
    ) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        context.gc()?;
        if self.revoke {
            unsafe { crate::sexp::instance::revoke_instance_availability(self.instance) };
        }
        if self.change_layout {
            crate::library::graphics::par::set_plot_parameter(
                "cex",
                crate::library::graphics::par::ParValue::Real(vec![2.0]),
            );
        }
        Ok(AltrepElement::Real(index as f64))
    }
}

#[test]
fn conversion_roots_input_across_altrep_gc_and_revocation() {
    for (revoke, change_layout) in [(false, false), (true, false), (false, true)] {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| {
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let calls = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class(
                    "convert-input",
                    Provider {
                        calls: calls.clone(),
                        revoke,
                        change_layout,
                        instance,
                    },
                )
                .unwrap();
            let input = AltrepBuilder::new(class).build().unwrap();
            let from = factory
                .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(2) })
                .unwrap();
            let to = factory
                .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(1) })
                .unwrap();
            let nil = factory.nil();
            let tail = factory.pairlist_cell(&to, &nil, &nil).unwrap();
            let tail = factory.pairlist_cell(&from, &tail, &nil).unwrap();
            let args = factory.pairlist_cell(&input, &tail, &nil).unwrap();
            unsafe {
                (*instance).graphics_recording = Some(Rc::new(RefCell::new(
                    r_graphics_engine::Scene::new(504, 504),
                )))
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                native_convert(args.as_raw(), false)
            }));
            assert!(calls.get() > 0);
            if revoke || change_layout {
                let error = outcome
                    .unwrap_err()
                    .downcast::<crate::sexp::context::RError>()
                    .unwrap();
                if change_layout {
                    assert_eq!(error.message, "graphics state changed during conversion");
                }
            } else {
                let result = owner.sexp(outcome.unwrap()).unwrap();
                assert_eq!(result.try_real_elt(0).unwrap(), 0.0);
                assert_eq!(result.try_real_elt(1).unwrap(), 504.0);
            }
        });
    }
}

#[test]
fn conversion_preserves_attributes_and_does_not_mutate_input() {
    use crate::sexp::object::SexpMut;
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let input = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 3)))
            .unwrap();
        let mut input = SexpMut::try_from_checked(input).unwrap();
        for (i, value) in [0.0, 0.5, 1.0].into_iter().enumerate() {
            input.try_set_real_elt(i as i64, value).unwrap();
        }
        let input = input.freeze();
        let names = factory.strings(&["α", "日本", "z"]).unwrap();
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                input.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
                names.as_raw(),
            )
        };
        let from = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(2) })
            .unwrap();
        let to = factory
            .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(1) })
            .unwrap();
        let nil = factory.nil();
        let tail = factory.pairlist_cell(&to, &nil, &nil).unwrap();
        let tail = factory.pairlist_cell(&from, &tail, &nil).unwrap();
        let args = factory.pairlist_cell(&input, &tail, &nil).unwrap();
        unsafe {
            (*instance).graphics_recording = Some(Rc::new(RefCell::new(
                r_graphics_engine::Scene::new(504, 504),
            )))
        };
        let result = owner
            .sexp(unsafe { native_convert(args.as_raw(), false) })
            .unwrap();
        assert_ne!(result.as_raw(), input.as_raw());
        for (i, expected) in [0.0, 252.0, 504.0].into_iter().enumerate() {
            assert_eq!(result.try_real_elt(i as i64).unwrap(), expected);
            assert_eq!(input.try_real_elt(i as i64).unwrap(), [0.0, 0.5, 1.0][i]);
        }
        let result_names = unsafe {
            crate::sexp::attrib_core::getAttrib(
                result.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            )
        };
        let result_names = owner.sexp(result_names).unwrap();
        for (i, expected) in ["α", "日本", "z"].into_iter().enumerate() {
            assert!(
                result_names
                    .try_string_elt(i as i64)
                    .unwrap()
                    .try_char_eq(expected.as_bytes())
                    .unwrap()
            );
        }
    });
}
