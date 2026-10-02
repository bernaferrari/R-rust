#![allow(unsafe_code)]
//! Immutable class configuration and callback leases. No interpreter field
//! borrow survives a call into provider or native code.
use super::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

/// Only vector representations with checked readers and writers are admitted.
#[derive(Clone, Copy, Debug)]
pub(super) enum VectorKind {
    Integer,
    Real,
    Logical,
    Raw,
    Complex,
    String,
    List,
}
impl VectorKind {
    pub(super) fn from_sexp(kind: SEXPTYPE) -> SexpResult<Self> {
        Ok(match kind {
            SEXPTYPE::INTSXP => Self::Integer,
            SEXPTYPE::REALSXP => Self::Real,
            SEXPTYPE::LGLSXP => Self::Logical,
            SEXPTYPE::RAWSXP => Self::Raw,
            SEXPTYPE::CPLXSXP => Self::Complex,
            SEXPTYPE::STRSXP => Self::String,
            SEXPTYPE::VECSXP => Self::List,
            _ => return Err(failure("unsupported ALTREP vector type")),
        })
    }
    pub(super) fn sexp_type(self) -> SEXPTYPE {
        match self {
            Self::Integer => SEXPTYPE::INTSXP,
            Self::Real => SEXPTYPE::REALSXP,
            Self::Logical => SEXPTYPE::LGLSXP,
            Self::Raw => SEXPTYPE::RAWSXP,
            Self::Complex => SEXPTYPE::CPLXSXP,
            Self::String => SEXPTYPE::STRSXP,
            Self::List => SEXPTYPE::VECSXP,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum CachePolicy {
    Private,
    Data2,
}
type NativeMethods = Rc<RefCell<crate::mainutils::altrep::NativeMethods>>;

/// Configuration is sampled before insertion, under the original owner.
/// It cannot drift even when a provider uses interior mutability.
pub(super) struct RegisteredClass {
    pub(super) kind: VectorKind,
    pub(super) cache: CachePolicy,
    pub(super) provider: Rc<dyn AltrepClass>,
    native: Option<NativeMethods>,
}
#[derive(Default)]
pub(crate) struct AltrepRuntimeState {
    classes: HashMap<usize, Rc<RegisteredClass>>,
    active: Rc<RefCell<HashSet<Operation>>>,
}

pub(super) fn register<'s>(
    owner: OwnerToken<'s>,
    name: &str,
    provider: Rc<dyn AltrepClass>,
) -> SexpResult<AltrepClassHandle<'s>> {
    register_record(owner, name, provider, None)
}
pub(crate) fn register_native<'s>(
    owner: OwnerToken<'s>,
    name: &str,
    provider: Rc<dyn AltrepClass>,
    methods: NativeMethods,
) -> SexpResult<AltrepClassHandle<'s>> {
    register_record(owner, name, provider, Some(methods))
}
fn register_record<'s>(
    owner: OwnerToken<'s>,
    name: &str,
    provider: Rc<dyn AltrepClass>,
    native: Option<NativeMethods>,
) -> SexpResult<AltrepClassHandle<'s>> {
    let kind = storage::activate(owner, || VectorKind::from_sexp(provider.vector_type()))?;
    let cache = storage::activate(owner, || {
        if provider.cache_in_data2() {
            CachePolicy::Data2
        } else {
            CachePolicy::Private
        }
    });
    let name = CString::new(format!(".AltrepClass.{name}"))
        .map_err(|_| failure("invalid ALTREP class name"))?;
    let descriptor = storage::intern(owner, &name)?;
    let key = descriptor.clone().as_raw() as usize;
    let class = Rc::new(RegisteredClass {
        kind,
        cache,
        provider,
        native,
    });
    // SAFETY: the capability retains this owner. Only this field is borrowed,
    // and the borrow ends before any callback, allocation of R nodes or GC.
    unsafe {
        let table = &mut (*owner.as_ptr()).altrep_state.classes;
        if table.contains_key(&key) {
            return Err(failure("ALTREP class already registered"));
        }
        table
            .try_reserve(1)
            .map_err(|_| failure("ALTREP class table"))?;
        table.insert(key, class);
    }
    Ok(AltrepClassHandle { owner, descriptor })
}
pub(super) fn lookup(owner: OwnerToken<'_>, descriptor: SEXP) -> Option<Rc<RegisteredClass>> {
    // SAFETY: copied Rc escapes; no interpreter reference escapes this read.
    unsafe {
        (*owner.as_ptr())
            .altrep_state
            .classes
            .get(&(descriptor as usize))
            .cloned()
    }
}
pub(crate) fn class_handle<'s>(
    owner: OwnerToken<'s>,
    raw: SEXP,
) -> SexpResult<AltrepClassHandle<'s>> {
    let descriptor = owner.sexp(raw)?;
    lookup(owner, descriptor.clone().as_raw()).ok_or(failure("unregistered ALTREP class"))?;
    Ok(AltrepClassHandle { owner, descriptor })
}
pub(crate) fn native_methods_for_class(class: &AltrepClassHandle<'_>) -> Option<NativeMethods> {
    lookup(class.owner, class.descriptor.clone().as_raw())?
        .native
        .clone()
}
pub(crate) fn native_methods(object: &Sexp<'_>) -> Option<NativeMethods> {
    let owner = storage::owner(object).ok()?;
    let descriptor = storage::Metadata::load(object)?.descriptor().ok()?;
    lookup(owner, descriptor.as_raw())?.native.clone()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Operation {
    Read(usize, i64),
    Expand(usize),
    Serialize(usize),
    Duplicate(usize),
}
/// Owns Rust state independently of the interpreter allocation.
pub(crate) struct OperationGuard {
    active: Rc<RefCell<HashSet<Operation>>>,
    operation: Operation,
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.operation);
    }
}
pub(super) fn enter_operation(
    owner: OwnerToken<'_>,
    operation: Operation,
) -> SexpResult<OperationGuard> {
    // SAFETY: end field access before inserting or invoking class code.
    let active = unsafe { (*owner.as_ptr()).altrep_state.active.clone() };
    {
        let mut state = active.borrow_mut();
        state
            .try_reserve(1)
            .map_err(|_| failure("callback state allocation"))?;
        if !state.insert(operation) {
            return Err(failure("recursive ALTREP operation"));
        }
    }
    Ok(OperationGuard { active, operation })
}
#[cfg(test)]
pub(super) fn operations_are_idle(owner: OwnerToken<'_>) -> bool {
    // SAFETY: the check does not invoke a callback or retain an instance borrow.
    unsafe { (*owner.as_ptr()).altrep_state.active.borrow().is_empty() }
}
