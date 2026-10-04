//! A real lazy binding remains lazy until public mget requests its value.
use crate::sexp::{RSession, ffi::SEXPTYPE, object::Sexp};

fn setup(expression: &str) -> (RSession, Sexp<'static>) {
    let mut session = RSession::new_for_gc_tests();
    session
        .eval_script_with_output_capture("counter<-0L;e<-new.env();e$y<-1L")
        .0
        .unwrap();
    let code = session
        .eval_code_with_output_capture(&format!("quote({expression})"))
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let env = session
        .eval_code_with_output_capture("e")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let promise = session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let global = owner.sexp(crate::sexp::globals::R_GlobalEnv()).unwrap();
        let promise = factory
            .promise(&code, &global)
            .unwrap()
            .into_owned()
            .unwrap();
        let symbol = owner
            .sexp(crate::sexp::symbol::Rf_install(c"x".as_ptr()))
            .unwrap();
        assert!(crate::sexp::envir::define_var_safe(
            symbol,
            promise.clone(),
            env
        ));
        promise
    });
    let count = session.eval_code_with_output_capture("counter").0.unwrap();
    assert_eq!(count.try_integer_elt(0).unwrap(), 0);
    drop(count);
    (session, promise)
}

#[test]
fn owning_mget_public_forces_real_lazy_binding_once() {
    let (mut session, promise) = setup("{counter<<-counter+1L;42L}");
    let values = session
        .eval_code_with_output_capture("mget('x',e,inherits=FALSE)")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let value = values.try_vector_elt(0).unwrap();
    assert_eq!(
        value.typeof_(),
        SEXPTYPE::INTSXP,
        "mget must return the forced value, not PROMSXP syntax"
    );
    assert_eq!(value.try_integer_elt(0).unwrap(), 42);
    assert_eq!(promise.try_prvalue().unwrap(), value);
    assert_eq!(
        session
            .eval_code_with_output_capture("counter")
            .0
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        1
    );
    let again = session
        .eval_code_with_output_capture("mget('x',e,inherits=FALSE)")
        .0
        .unwrap();
    assert_eq!(again.try_vector_elt(0).unwrap(), value);
    drop(again);
    assert_eq!(
        session
            .eval_code_with_output_capture("counter")
            .0
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        1
    );
}

#[test]
fn owning_mget_duplicate_names_use_the_same_cached_value() {
    let (mut session, _) = setup("{counter<<-counter+1L;42L}");
    let values = session
        .eval_code_with_output_capture("mget(c('x','x'),e,inherits=FALSE)")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let first = values.try_vector_elt(0).unwrap();
    assert_eq!(first.typeof_(), SEXPTYPE::INTSXP);
    assert_eq!(first, values.try_vector_elt(1).unwrap());
    assert_eq!(first.try_integer_elt(0).unwrap(), 42);
    assert_eq!(
        session
            .eval_code_with_output_capture("counter")
            .0
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        1
    );
}

#[test]
fn owning_mget_forcing_precedes_later_name_lookup() {
    let (mut session, _) = setup("{counter<<-counter+1L;e$y<-9L;42L}");
    let values = session
        .eval_code_with_output_capture("mget(c('x','y'),e,inherits=FALSE)")
        .0
        .unwrap();
    assert_eq!(
        values
            .try_vector_elt(0)
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        42
    );
    assert_eq!(
        values
            .try_vector_elt(1)
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        9
    );
}

use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::{R_xlen_t, SEXP},
    heap::CheckedNode,
    object::{SessionNodeFactory, SexpResult},
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct CollectingNames {
    arguments: Rc<Cell<SEXP>>,
    identities: Vec<CheckedNode>,
    calls: Rc<Cell<usize>>,
    session: Weak<RefCell<Option<RSession>>>,
    close: bool,
}
impl AltrepClass for CollectingNames {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
        Ok(2)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: R_xlen_t,
    ) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let text = context.string(if index == 0 { "x" } else { "missing" })?;
        if index == 0 {
            unsafe {
                crate::sexp::accessors::SETCDR(
                    self.arguments.get(),
                    crate::sexp::globals::R_NilValue(),
                );
            }
            context.gc()?;
            for identity in &self.identities {
                assert!(
                    identity.is_live(),
                    "detached mget operand graph must survive real collection"
                );
            }
            if self.close {
                self.session
                    .upgrade()
                    .unwrap()
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .close();
            }
        }
        Ok(AltrepElement::String(text))
    }
}
fn mget_arguments(
    factory: &SessionNodeFactory<'_>,
    entries: Vec<(Sexp<'static>, Sexp<'static>)>,
) -> Sexp<'static> {
    let mut result = factory.nil().into_owned().unwrap();
    for (value, tag) in entries.into_iter().rev() {
        result = factory
            .pairlist_cell(&value, &result, &tag)
            .unwrap()
            .into_owned()
            .unwrap();
    }
    result
}
fn exercise_collecting_names(close: bool, detach_binding_during_force: bool) {
    let (mut session, promise) = setup("{gc();counter<<-counter+1L;42L}");
    let environment = session
        .eval_code_with_output_capture("e")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let factory: SessionNodeFactory<'static> = session
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap()
        .node_factory()
        .unwrap();
    let global = unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::globals::R_GlobalEnv())
            .unwrap()
            .into_owned()
            .unwrap()
    };
    let symbol = |name: &std::ffi::CStr| unsafe {
        session
            .owner_token()
            .unwrap()
            .sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))
            .unwrap()
            .into_owned()
            .unwrap()
    };
    let e = symbol(c"e");
    let x = symbol(c"x");
    crate::sexp::envir::remove_binding_raw(global.as_raw(), e.as_raw());
    let fallback = factory
        .strings(&["fallback"])
        .unwrap()
        .into_owned()
        .unwrap();
    let defaults = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
        .unwrap()
        .into_owned()
        .unwrap();
    let mut defaults = crate::sexp::object::SexpMut::try_from_checked(defaults).unwrap();
    defaults.try_set_vector_elt(0, fallback.clone()).unwrap();
    let defaults = defaults.freeze();
    let identities = vec![
        environment.allocation().unwrap().clone(),
        promise.allocation().unwrap().clone(),
        defaults.allocation().unwrap().clone(),
        fallback.allocation().unwrap().clone(),
    ];
    let promise_identity = promise.allocation().unwrap().clone();
    let environment_ptr = environment.as_raw();
    let symbol_ptr = x.as_raw();
    let sessions = Rc::new(RefCell::new(Some(session)));
    let argument_pointer = Rc::new(Cell::new(std::ptr::null_mut()));
    let calls = Rc::new(Cell::new(0));
    let class = sessions
        .borrow()
        .as_ref()
        .unwrap()
        .register_altrep_class(
            "collecting_mget_names",
            CollectingNames {
                arguments: argument_pointer.clone(),
                identities,
                calls: calls.clone(),
                session: Rc::downgrade(&sessions),
                close,
            },
        )
        .unwrap()
        .into_owned()
        .unwrap();
    let names = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let nil = factory.nil().into_owned().unwrap();
    let list = mget_arguments(
        &factory,
        vec![
            (names, nil.clone()),
            (environment, nil.clone()),
            (
                factory.strings(&["any"]).unwrap().into_owned().unwrap(),
                nil.clone(),
            ),
            (defaults, nil),
        ],
    );
    argument_pointer.set(list.as_raw());
    drop(promise);
    drop(fallback);
    drop(global);
    drop(e);
    drop(x);
    let collections = Rc::new(Cell::new(0));
    let collected = collections.clone();
    let detached = Rc::new(Cell::new(false));
    let detached_callback = detached.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        collected.set(collected.get() + 1);
        if detach_binding_during_force && collected.get() == 2 && !detached_callback.replace(true) {
            crate::sexp::envir::remove_binding_raw(environment_ptr, symbol_ptr);
            unsafe {
                crate::sexp::owner::OwnerToken::current()
                    .unwrap()
                    .full_gc()
                    .unwrap();
            }
            assert!(
                promise_identity.is_live(),
                "selected promise must remain owned after its binding is removed"
            );
        }
    }));
    // No runtime facade loan or incidental operand lease crosses the real provider.
    let result = unsafe { super::mget::invoke(list.as_raw()) };
    if close {
        assert!(result.is_err(), "original revocation must deny publication");
        assert!(!sessions.borrow().as_ref().unwrap().is_active());
        let mut replacement = RSession::new_for_gc_tests();
        assert_eq!(
            replacement
                .eval_code_with_output_capture("1L+1L")
                .0
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            2
        );
    } else {
        let result = result.unwrap();
        assert_eq!(
            result
                .try_vector_elt(0)
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            42
        );
        assert_eq!(
            result
                .try_vector_elt(1)
                .unwrap()
                .try_string_elt(0)
                .unwrap()
                .try_as_string()
                .unwrap(),
            "fallback"
        );
        assert_eq!(calls.get(), 2);
        if detach_binding_during_force {
            assert!(
                detached.get(),
                "actual promise evaluation must collect and remove its binding"
            );
        }
    }
    assert!(collections.get() > 0, "real full collection is required");
}
#[test]
fn owning_mget_name_provider_detaches_operands_and_collects() {
    exercise_collecting_names(false, false);
}
#[test]
fn owning_mget_selected_promise_survives_binding_detach_and_collection() {
    exercise_collecting_names(false, true);
}
#[test]
fn owning_mget_name_provider_revocation_denies_publication() {
    exercise_collecting_names(true, false);
}
