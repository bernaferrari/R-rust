use crate::sexp::RSession;

#[test]
fn owned_parent_environment_admission_matches_independent_gnu() {
    let mut session = RSession::new_without_default_packages();
    for line in include_str!("gnu-r90451.tsv").lines().skip(1) {
        let (expression, expected) = line.split_once('\t').unwrap();
        let code = format!(
            "tryCatch({{value <- {expression}; if(is.logical(value)) as.character(value) else 'OK'}}, error=function(e) conditionMessage(e))"
        );
        let (value, _, _) = session.eval_code_with_output_capture(&code);
        let actual = value
            .unwrap_or_else(|error| panic!("{expression}: {}", error.message))
            .try_string_elt(0)
            .unwrap()
            .try_as_string()
            .unwrap();
        assert_eq!(actual, expected, "{expression}");
    }
}

#[test]
fn owned_base_bootstrap_seals_original_shared_bindings() {
    let mut session = RSession::new_without_default_packages();
    let (value, _, _) = session.eval_code_with_output_capture("environmentIsLocked(baseenv()) && environmentIsLocked(asNamespace('base')) && bindingIsLocked('sum', baseenv()) && bindingIsLocked('sum', asNamespace('base')) && !bindingIsLocked('.Device', baseenv()) && !bindingIsLocked('.Devices', baseenv())");
    assert_eq!(value.unwrap().try_logical_elt(0).unwrap(), 1);
}

use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::{R_xlen_t, SEXP, SEXPTYPE},
    heap::CheckedNode,
    object::SexpResult,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct ParentNameProvider {
    arguments: Rc<Cell<SEXP>>,
    original: Rc<RefCell<Option<CheckedNode>>>,
    sessions: Weak<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
    close: bool,
}
impl AltrepClass for ParentNameProvider {
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
        unsafe {
            crate::sexp::accessors::SETCAR(
                self.arguments.get(),
                crate::sexp::globals::R_NilValue(),
            );
            crate::sexp::accessors::SETCDR(
                self.arguments.get(),
                crate::sexp::globals::R_NilValue(),
            );
        }
        context.gc()?;
        assert!(
            self.original.borrow().as_ref().unwrap().is_live(),
            "selected environment must be owned before the name provider detaches arguments and collects"
        );
        if self.close {
            self.sessions
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
            crate::sexp::context::r_error("provider signal after closing original runtime");
        }
        Ok(AltrepElement::String(context.string("ordinary")?))
    }
}

fn provider_case(close: bool) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let arguments = Rc::new(Cell::new(std::ptr::null_mut()));
    let original = Rc::new(RefCell::new(None));
    let calls = Rc::new(Cell::new(0));
    let (args, parent) = {
        let session = sessions.borrow();
        let session = session.as_ref().unwrap();
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let nil = factory.nil().into_owned().unwrap();
        let env = unsafe {
            owner
                .sexp(crate::sexp::memory_ext::NewEnvironment(
                    nil.as_raw(),
                    crate::sexp::envir::R_BaseNamespace(),
                    nil.as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let parent = unsafe {
            owner
                .sexp(crate::sexp::memory_ext::NewEnvironment(
                    nil.as_raw(),
                    crate::sexp::globals::R_EmptyEnv(),
                    nil.as_raw(),
                ))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        *original.borrow_mut() = Some(env.allocation().unwrap().clone()); // identity, no root
        let class = session
            .register_altrep_class(
                "parent_env_name",
                ParentNameProvider {
                    arguments: arguments.clone(),
                    original: original.clone(),
                    sessions: Rc::downgrade(&sessions),
                    calls: calls.clone(),
                    close,
                },
            )
            .unwrap()
            .into_owned()
            .unwrap();
        let name = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                env.as_raw(),
                crate::sexp::symbol::Rf_install(c"name".as_ptr()),
                name.as_raw(),
            );
            crate::sexp::envir::lock_environment_raw(env.as_raw());
        }
        let tail = factory
            .pairlist_cell(&parent, &nil, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        let args = factory
            .pairlist_cell(&env, &tail, &nil)
            .unwrap()
            .into_owned()
            .unwrap();
        arguments.set(args.as_raw());
        (args, parent)
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        super::dispatch(args.as_raw(), true)
    }));
    assert_eq!(calls.get(), 1);
    if close {
        let error = result.unwrap_err();
        let error = error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap();
        assert_eq!(
            error.message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        );
    } else {
        let value = result.unwrap();
        let owner = unsafe { crate::sexp::owner::OwnerToken::current().unwrap() };
        let value = owner.sexp(value).unwrap().into_owned().unwrap();
        assert_eq!(value.try_enclos().unwrap().as_raw(), parent.as_raw());
    }
}

#[test]
fn owned_parent_environment_survives_detached_arguments_and_name_collection() {
    provider_case(false);
}

#[test]
fn owned_parent_environment_rejects_original_revocation_after_provider_error() {
    provider_case(true);
}
