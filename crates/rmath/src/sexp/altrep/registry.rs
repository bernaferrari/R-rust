#![forbid(unsafe_code)]
//! Immutable class configuration and callback leases. No interpreter field
//! borrow survives a call into provider code.
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

/// Configuration is sampled before insertion, under the original owner.
/// It cannot drift even when a provider uses interior mutability.
pub(super) struct RegisteredClass {
    pub(super) kind: VectorKind,
    pub(super) cache: CachePolicy,
    pub(super) provider: Rc<dyn AltrepClass>,
    pub(super) builtin_sequence: Option<SEXPTYPE>,
}
#[derive(Clone, Default)]
pub(crate) struct AltrepRuntimeState {
    inner: Rc<RuntimeInner>,
}
#[derive(Default)]
struct RuntimeInner {
    classes: RefCell<HashMap<usize, Rc<RegisteredClass>>>,
    active: RefCell<HashSet<Operation>>,
}
impl AltrepRuntimeState {
    fn insert(&self, key: usize, class: Rc<RegisteredClass>) -> SexpResult<()> {
        // No provider code runs while this checked Rust borrow is live.
        let mut table = self.inner.classes.borrow_mut();
        if table.contains_key(&key) {
            return Err(failure("ALTREP class already registered"));
        }
        table
            .try_reserve(1)
            .map_err(|_| failure("ALTREP class table"))?;
        table.insert(key, class);
        Ok(())
    }
    fn lookup(&self, key: usize) -> Option<Rc<RegisteredClass>> {
        self.inner.classes.borrow().get(&key).cloned()
    }
    pub(super) fn enter_operation(&self, operation: Operation) -> SexpResult<OperationGuard> {
        {
            let mut active = self.inner.active.borrow_mut();
            active
                .try_reserve(1)
                .map_err(|_| failure("callback state allocation"))?;
            if !active.insert(operation) {
                return Err(failure("recursive ALTREP operation"));
            }
        }
        Ok(OperationGuard {
            state: self.clone(),
            operation,
        })
    }
    #[cfg(test)]
    pub(super) fn operations_are_idle(&self) -> bool {
        self.inner.active.borrow().is_empty()
    }
}

pub(super) fn register<'s>(
    owner: OwnerToken<'s>,
    name: &str,
    provider: Rc<dyn AltrepClass>,
) -> SexpResult<AltrepClassHandle<'s>> {
    if name.starts_with(".builtin.") {
        return Err(failure("reserved built-in ALTREP class name"));
    }
    register_impl(owner, name, provider, None)
}

pub(super) fn register_builtin_sequence<'s>(
    owner: OwnerToken<'s>,
    kind: SEXPTYPE,
) -> SexpResult<AltrepClassHandle<'s>> {
    let name = match kind {
        SEXPTYPE::INTSXP => ".builtin.compact_intseq",
        SEXPTYPE::REALSXP => ".builtin.compact_realseq",
        _ => return Err(failure("sequence vector type")),
    };
    register_impl(
        owner,
        name,
        Rc::new(super::builtins::SequenceClass(kind)),
        Some(kind),
    )
}

fn register_impl<'s>(
    owner: OwnerToken<'s>,
    name: &str,
    provider: Rc<dyn AltrepClass>,
    builtin_sequence: Option<SEXPTYPE>,
) -> SexpResult<AltrepClassHandle<'s>> {
    let capability = StoredOwner::from_token(owner);
    storage::with_owner(&capability, |owner| {
        let kind = storage::invoke_provider(&capability, || {
            VectorKind::from_sexp(provider.vector_type())
        })?;
        let cache = storage::invoke_provider(&capability, || {
            Ok({
                if provider.cache_in_data2() {
                    CachePolicy::Data2
                } else {
                    CachePolicy::Private
                }
            })
        })?;
        let name = CString::new(format!(".AltrepClass.{name}"))
            .map_err(|_| failure("invalid ALTREP class name"))?;
        let descriptor = storage::intern(owner, &name)?;
        let descriptor = capability.sexp(descriptor.as_raw())?;
        if let Some(builtin_kind) = builtin_sequence {
            if kind.sexp_type() != builtin_kind {
                return Err(failure("built-in class type mismatch"));
            }
            let node = descriptor.allocation()?;
            node.heap_identity()
                .attach_builtin_sequence_permit(node, builtin_kind)
                .ok_or(failure("built-in class permit admission"))?;
        }
        let key = descriptor.clone().as_raw() as usize;
        let class = Rc::new(RegisteredClass {
            kind,
            cache,
            provider,
            builtin_sequence,
        });
        bridge::runtime(owner).insert(key, class.clone())?;
        Ok(AltrepClassHandle {
            owner: capability.clone(),
            descriptor,
            record: class,
        })
    })
}
pub(super) fn lookup(owner: OwnerToken<'_>, descriptor: SEXP) -> Option<Rc<RegisteredClass>> {
    bridge::runtime(owner).lookup(descriptor as usize)
}

pub(crate) fn class_handle<'s>(
    owner: OwnerToken<'s>,
    raw: SEXP,
) -> SexpResult<AltrepClassHandle<'s>> {
    let descriptor = owner.sexp(raw)?;
    let record =
        lookup(owner, descriptor.clone().as_raw()).ok_or(failure("unregistered ALTREP class"))?;
    Ok(AltrepClassHandle {
        owner: StoredOwner::from_token(owner),
        descriptor,
        record,
    })
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
    state: AltrepRuntimeState,
    operation: Operation,
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.state.inner.active.borrow_mut().remove(&self.operation);
    }
}
pub(super) fn enter_operation(
    owner: OwnerToken<'_>,
    operation: Operation,
) -> SexpResult<OperationGuard> {
    bridge::runtime(owner).enter_operation(operation)
}
#[cfg(test)]
pub(super) fn operations_are_idle(owner: OwnerToken<'_>) -> bool {
    bridge::runtime(owner).operations_are_idle()
}
