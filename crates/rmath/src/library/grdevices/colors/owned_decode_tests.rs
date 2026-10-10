//! Genuine original-owner string decoding, including NA and provider revocation.
use super::{R_TRANWHITE, inRGBpar3};
use crate::sexp::{
    RSession,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::{R_xlen_t, SEXPTYPE},
    object::SexpResult,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct CollectingColor {
    sessions: Weak<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
    close: bool,
}
impl AltrepClass for CollectingColor {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        _: R_xlen_t,
    ) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let color = context.string("blue")?;
        context.gc()?;
        assert_eq!(
            color.try_as_string()?,
            "blue",
            "fresh selected color survives genuine full GC"
        );
        if self.close {
            self.sessions
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }
        Ok(AltrepElement::String(color))
    }
}
fn provider_case(close: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let color = {
        let session = sessions.borrow();
        let session = session.as_ref().unwrap();
        let class = session
            .register_altrep_class(
                "collecting_plot_color",
                CollectingColor {
                    sessions: Rc::downgrade(&sessions),
                    calls: calls.clone(),
                    close,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap()
    };
    // No runtime facade or RefCell loan crosses the actual provider callback.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        inRGBpar3(color.as_raw(), 0, R_TRANWHITE)
    }));
    assert_eq!(calls.get(), 1);
    if close {
        let error = result.expect_err("revoked original owner must reject decoded color");
        assert!(
            error.is::<crate::sexp::context::RError>(),
            "typed R error, not a decoder panic"
        );
    } else {
        assert_eq!(
            result.unwrap(),
            0xffff0000,
            "independent GNU blue RGBA encoding"
        );
    }
}
#[test]
fn owning_color_na_character_is_transparent() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .eval_code_with_output_capture("NA_character_")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    assert_eq!(
        unsafe { inRGBpar3(value.as_raw(), 0, 0xff000000) },
        R_TRANWHITE
    );
}
#[test]
fn owning_color_selected_character_survives_collecting_provider() {
    provider_case(false);
}
#[test]
fn owning_color_revoked_provider_denies_color_publication() {
    provider_case(true);
}
