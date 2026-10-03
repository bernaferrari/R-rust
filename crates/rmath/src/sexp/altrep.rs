#![deny(unsafe_code)]
//! Rooted, typed ALTREP classes. Class code uses copied values and never loans
//! an R payload. Private storage and bridge modules own raw publication; class
//! methods may allocate, collect or reenter R without a live arena borrow.
//!
//! Class descriptors are interned symbols. Instance data are ordinary GC-traced
//! VECSXP slots in an internal attribute, independent of the vector's type and
//! logical length. Rust method tables stay in the owning session.

use super::{
    ffi::{R_xlen_t, Rcomplex, SEXP, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::{OwnerToken, StoredOwner},
    session::{with_instance_active, RSession},
};
use std::{ffi::CString, rc::Rc};

mod bridge;
mod registry;
mod storage;
pub(crate) use bridge::{has_extension_raw, lazy_raw, materialize_raw, rooted_raw};
pub(crate) use registry::{class_handle, AltrepRuntimeState, OperationGuard};
use registry::{enter_operation, lookup, register, Operation, RegisteredClass};
use storage::{allocate, owner, InstanceStorage, Metadata};

/// A copied element, or an independently rooted string/list element.
#[derive(Debug)]
pub enum AltrepElement<'s> {
    Integer(i32),
    Real(f64),
    Logical(i32),
    Raw(u8),
    Complex(Rcomplex),
    String(Sexp<'s>),
    List(Sexp<'s>),
}

/// Per-instance R values belong in GC-traced data1/data2. Providers are static
/// Rust code and cannot retain borrowed session handles.
/// Type and cache policy are sampled once at registration; length is fixed at
/// construction. Element callbacks may run R or collect; they return copied
/// scalars or rooted child handles.
pub trait AltrepClass: 'static {
    fn vector_type(&self) -> SEXPTYPE;
    /// Built-ins may expose the completed dense cache as R's data2 field.
    fn cache_in_data2(&self) -> bool {
        false
    }
    fn length(&self, context: &AltrepContext<'_>) -> SexpResult<R_xlen_t>;
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: R_xlen_t,
    ) -> SexpResult<AltrepElement<'s>>;
}

/// Lifetime-bound activation and allocation for a class callback.
/// No method exposes a borrowed R buffer or a mutable RInstance reference.
pub struct AltrepContext<'s> {
    owner: StoredOwner<'s>,
    object: Sexp<'s>,
    metadata: Metadata<'s>,
}
impl<'s> AltrepContext<'s> {
    pub(crate) fn wrap(&self, raw: SEXP) -> SexpResult<Sexp<'s>> {
        self.owner.sexp(raw)
    }
    pub fn object(&self) -> Sexp<'s> {
        self.object.clone()
    }
    /// Read current traced data and install an independent root for its use.
    pub fn data1(&self) -> SexpResult<Sexp<'s>> {
        self.metadata.data1()
    }
    /// Cache writes are immediately visible, including within one callback.
    pub fn data2(&self) -> SexpResult<Sexp<'s>> {
        self.metadata.data2()
    }
    pub fn set_data2(&self, value: Sexp<'s>) -> SexpResult<()> {
        self.metadata.set_data2(self.owner.sexp(value.as_raw())?)
    }
    pub fn gc(&self) -> SexpResult<()> {
        storage::with_owner(&self.owner, |owner| {
            storage::activate(owner, || owner.full_gc().map(|_| ()))
        })
    }
    pub fn alloc_vector(&self, kind: SEXPTYPE, length: R_xlen_t) -> SexpResult<Sexp<'s>> {
        storage::with_owner(&self.owner, |owner| {
            let value = allocate(owner, kind, length)?;
            self.owner.sexp(value.as_raw())
        })
    }
    pub fn string(&self, text: &str) -> SexpResult<Sexp<'s>> {
        storage::with_owner(&self.owner, |owner| {
            let value = storage::string(owner, text)?;
            self.owner.sexp(value.as_raw())
        })
    }
    pub fn eval(&self, expression: Sexp<'s>, environment: Sexp<'s>) -> SexpResult<Sexp<'s>> {
        storage::with_owner(&self.owner, |owner| {
            let expression = owner.sexp(expression.as_raw())?;
            let environment = owner.sexp(environment.as_raw())?;
            let value = bridge::eval(owner, expression, environment)?;
            self.owner.sexp(value.as_raw())
        })
    }
    fn active<T>(&self, f: impl FnOnce() -> SexpResult<T>) -> SexpResult<T> {
        storage::with_owner(&self.owner, |owner| storage::activate(owner, f))
    }
}

/// A session-bound class token. A descriptor cannot accidentally select a class
/// in a different session, or disappear while an instance is being built.
#[derive(Clone)]
pub struct AltrepClassHandle<'s> {
    owner: StoredOwner<'s>,
    descriptor: Sexp<'s>,
    record: Rc<RegisteredClass>,
}
impl<'s> AltrepClassHandle<'s> {
    pub fn descriptor(&self) -> Sexp<'s> {
        self.descriptor.clone()
    }

    /// Retain the actual descriptor and weak authority independently of a
    /// session borrow. Construction fails after the original runtime closes.
    pub fn into_owned(self) -> SexpResult<AltrepClassHandle<'static>> {
        Ok(AltrepClassHandle {
            owner: self.owner.into_owned()?,
            descriptor: self.descriptor.into_owned()?,
            record: self.record,
        })
    }
}

impl RSession {
    pub fn register_altrep_class(
        &self,
        name: &str,
        class: impl AltrepClass,
    ) -> SexpResult<AltrepClassHandle<'_>> {
        let owner = self.owner_token().ok_or(SexpError::OwnerNotActive)?;
        if name.starts_with(".builtin.") {
            return Err(failure("reserved built-in class name"));
        }
        register(owner, name, Rc::new(class))
    }
}

fn failure(what: &'static str) -> SexpError {
    SexpError::Altrep { reason: what }
}

/// Construct using two rooted data objects; builder order has no effect.
pub struct AltrepBuilder<'s> {
    class: AltrepClassHandle<'s>,
    data1: Sexp<'s>,
    data2: Sexp<'s>,
}
impl<'s> AltrepBuilder<'s> {
    pub fn new(class: AltrepClassHandle<'s>) -> Self {
        let nil = class
            .descriptor
            .original_singleton_nil()
            .unwrap_or_else(Sexp::nil);
        Self {
            class,
            data1: nil.clone(),
            data2: nil,
        }
    }
    pub fn data1(mut self, data: Sexp<'s>) -> Self {
        self.data1 = data;
        self
    }
    pub fn data2(mut self, data: Sexp<'s>) -> Self {
        self.data2 = data;
        self
    }
    pub fn build(self) -> SexpResult<Sexp<'s>> {
        let class = self.class.record.clone();
        let pending = InstanceStorage::create(&self.class, class.kind, self.data1, self.data2)?;
        let context = pending.context();
        let length = context.active(|| class.provider.length(&context))?;
        pending.finish(length)
    }
}

fn context<'s>(object: &Sexp<'s>) -> SexpResult<(AltrepContext<'s>, Rc<RegisteredClass>)> {
    let owner = StoredOwner::from_value(object)?;
    let data = Metadata::load(object).ok_or(failure("invalid ALTREP metadata"))?;
    let descriptor = data.descriptor()?;
    let class = storage::with_owner(&owner, |token| {
        lookup(token, descriptor.as_raw()).ok_or(failure("unregistered ALTREP class"))
    })?;
    if class.kind.sexp_type() != object.typeof_() {
        return Err(failure("ALTREP class type mismatch"));
    }
    Ok((
        AltrepContext {
            owner,
            object: object.clone(),
            metadata: data,
        },
        class,
    ))
}

pub(crate) fn data1<'s>(object: &Sexp<'s>) -> SexpResult<Sexp<'s>> {
    Metadata::load(object)
        .ok_or(failure("invalid ALTREP metadata"))?
        .data1()
}
pub(crate) fn data2<'s>(object: &Sexp<'s>) -> SexpResult<Sexp<'s>> {
    Metadata::load(object)
        .ok_or(failure("invalid ALTREP metadata"))?
        .data2()
}
pub(crate) fn set_data1<'s>(object: &Sexp<'s>, value: Sexp<'s>) -> SexpResult<()> {
    let value = owner(object)?.sexp(value.as_raw())?;
    Metadata::load(object)
        .ok_or(failure("invalid ALTREP metadata"))?
        .set_data1(value)
}
pub(crate) fn set_data2<'s>(object: &Sexp<'s>, value: Sexp<'s>) -> SexpResult<()> {
    let value = owner(object)?.sexp(value.as_raw())?;
    Metadata::load(object)
        .ok_or(failure("invalid ALTREP metadata"))?
        .set_data2(value)
}

pub(crate) fn activate_for<T>(object: &Sexp<'_>, callback: impl FnOnce() -> T) -> SexpResult<T> {
    let pin = object.pin_runtime()?;
    let owner = owner(object)?;
    let result = storage::activate(owner, callback);
    if let Some(pin) = &pin {
        pin.require_live()?;
    }
    Ok(result)
}

pub fn is_altrep(object: &Sexp<'_>) -> bool {
    object.header().sxpinfo.alt()
}
pub fn is_materialized(object: &Sexp<'_>) -> bool {
    !object.header().payload.is_empty() || object.is_empty()
}
pub fn altrep_class<'s>(object: &Sexp<'s>) -> Option<Sexp<'s>> {
    Metadata::load(object)?.descriptor().ok()
}
pub fn altrep_length(object: &Sexp<'_>) -> R_xlen_t {
    object.len()
}

pub fn altrep_elt<'s>(object: &Sexp<'s>, index: R_xlen_t) -> SexpResult<AltrepElement<'s>> {
    if index < 0 || index >= object.len() {
        return Err(SexpError::OutOfBounds {
            index,
            len: object.len(),
        });
    }
    if is_materialized(object) || Metadata::load(object).is_none() {
        return dense_element(object, index);
    }
    let (context, class) = context(object)?;
    let storage = InstanceStorage::load(object)?;
    let value = invoke_element(&context, &*class, index)?;
    storage.validate()?;
    validate_element(&context, value)
}
fn validate_element<'s>(
    context: &AltrepContext<'s>,
    value: AltrepElement<'s>,
) -> SexpResult<AltrepElement<'s>> {
    let kind = context.object.typeof_();
    let valid = matches!(
        (&value, kind),
        (AltrepElement::Integer(_), SEXPTYPE::INTSXP)
            | (AltrepElement::Real(_), SEXPTYPE::REALSXP)
            | (AltrepElement::Logical(_), SEXPTYPE::LGLSXP)
            | (AltrepElement::Raw(_), SEXPTYPE::RAWSXP)
            | (AltrepElement::Complex(_), SEXPTYPE::CPLXSXP)
            | (AltrepElement::String(_), SEXPTYPE::STRSXP)
            | (AltrepElement::List(_), SEXPTYPE::VECSXP)
    );
    if !valid {
        return Err(failure("element type mismatch"));
    }
    Ok(match value {
        AltrepElement::String(v) => {
            let v = context.owner.sexp(v.as_raw())?;
            if v.typeof_() != SEXPTYPE::CHARSXP {
                return Err(failure("string element is not CHARSXP"));
            }
            AltrepElement::String(v)
        }
        AltrepElement::List(v) => AltrepElement::List(context.owner.sexp(v.as_raw())?),
        scalar => scalar,
    })
}
pub(crate) fn serialization_guard(object: &Sexp<'_>) -> SexpResult<OperationGuard> {
    let owner = StoredOwner::from_value(object)?;
    storage::with_owner(&owner, |token| {
        enter_operation(token, Operation::Serialize(object.as_raw() as usize))
    })
}

pub(crate) fn duplication_guard(object: &Sexp<'_>) -> SexpResult<OperationGuard> {
    let owner = StoredOwner::from_value(object)?;
    storage::with_owner(&owner, |token| {
        enter_operation(token, Operation::Duplicate(object.as_raw() as usize))
    })
}

fn invoke_element<'s>(
    context: &AltrepContext<'s>,
    class: &RegisteredClass,
    index: i64,
) -> SexpResult<AltrepElement<'s>> {
    storage::with_owner(&context.owner, |owner| {
        let _operation = enter_operation(
            owner,
            Operation::Read(context.object.as_raw() as usize, index),
        )?;
        context.active(|| class.provider.element(context, index))
    })
}
pub(crate) fn lazy_element<'s>(
    object: &Sexp<'s>,
    index: R_xlen_t,
) -> Option<SexpResult<AltrepElement<'s>>> {
    if !object.header().payload.is_empty() || Metadata::load(object).is_none() {
        return None;
    }
    Some(altrep_elt(object, index))
}
/// Retain pointer-valued native reads in the parent before returning a raw
/// SEXP. Safe Rust readers already return independent leases. The sparse
/// cache keeps native STRING_ELT/VECTOR_ELT results traced while the vector
/// stays lazy, without allocating a full-length pointer array.
fn retain_native_child<'s>(object: &Sexp<'s>, index: i64, child: &Sexp<'s>) -> SexpResult<()> {
    Metadata::load(object)
        .ok_or(failure("missing instance metadata"))?
        .retain_child(owner(object)?, index, child.clone())
}

fn dense_element<'s>(object: &Sexp<'s>, i: R_xlen_t) -> SexpResult<AltrepElement<'s>> {
    Ok(match object.typeof_() {
        SEXPTYPE::INTSXP => AltrepElement::Integer(object.try_integer_elt(i)?),
        SEXPTYPE::REALSXP => AltrepElement::Real(object.try_real_elt(i)?),
        SEXPTYPE::LGLSXP => AltrepElement::Logical(object.try_logical_elt(i)?),
        SEXPTYPE::RAWSXP => AltrepElement::Raw(object.try_raw_elt(i)?),
        SEXPTYPE::CPLXSXP => AltrepElement::Complex(object.try_complex_elt(i)?),
        SEXPTYPE::STRSXP => AltrepElement::String(object.try_string_elt(i)?),
        SEXPTYPE::VECSXP => AltrepElement::List(object.try_vector_elt(i)?),
        _ => return Err(failure("unsupported ALTREP type")),
    })
}

fn write_element<'s>(
    output: &mut SexpMut<'s>,
    i: R_xlen_t,
    value: AltrepElement<'s>,
) -> SexpResult<()> {
    match value {
        AltrepElement::Integer(v) if output.typeof_() == SEXPTYPE::INTSXP => {
            output.try_set_integer_elt(i, v)
        }
        AltrepElement::Real(v) => output.try_set_real_elt(i, v),
        AltrepElement::Logical(v) => output.try_set_logical_elt(i, v),
        AltrepElement::Raw(v) => output.try_set_raw_elt(i, v),
        AltrepElement::Complex(v) => output.try_set_complex_elt(i, v),
        AltrepElement::String(v) => output.try_set_string_elt(i, v),
        AltrepElement::List(v) => output.try_set_vector_elt(i, v),
        _ => Err(failure("ALTREP element type mismatch")),
    }
}

/// Expansion publishes a complete payload or leaves the lazy representation
/// available for retry. Callback side effects are not rolled back on failure.
/// Collection can trace the private output and metadata during each callback.
pub fn force_materialization(object: &Sexp<'_>) -> SexpResult<()> {
    if Metadata::load(object).is_none() || is_materialized(object) {
        return Ok(());
    }
    let (context, class) = context(object)?;
    storage::with_owner(&context.owner, |owner| {
        let storage = InstanceStorage::load(object)?;
        let _operation =
            enter_operation(owner, Operation::Expand(object.clone().as_raw() as usize))?;
        if super::memory::is_arena_lent(owner.as_ptr()) {
            return Err(failure("release the arena lend before materialization"));
        }
        let output = allocate(owner, object.typeof_(), object.len())?;
        let output = context.owner.sexp(output.as_raw())?;
        let mut output = SexpMut::try_from_checked(output)?;
        storage.validate()?;
        for i in 0..object.len() {
            let value = invoke_element(&context, &*class, i)?;
            storage.validate()?;
            write_element(&mut output, i, validate_element(&context, value)?)?;
        }
        let output = output.freeze();
        // Storage installs traced cache roots and a checked buffer lease together.
        storage.publish_dense(context.owner.sexp(output.as_raw())?, class.cache)?;
        Ok(())
    })
}

/// Create a dense value copy, preserving public attributes. Serialization uses
/// this fallback when a class does not have a portable serialized state.
pub fn materialized_copy<'s>(object: &Sexp<'s>) -> SexpResult<Sexp<'s>> {
    let _pin = object.pin_runtime()?;
    let owner = owner(object)?;
    let output = allocate(owner, object.typeof_(), object.len())?;
    let mut output = SexpMut::try_from_checked(output)?;
    for i in 0..object.len() {
        write_element(&mut output, i, altrep_elt(object, i)?)?;
    }
    let output = output.freeze();
    storage::copy_public_attributes(object, &output)?;
    Ok(output)
}

mod builtins;
pub(crate) use builtins::{builtin_sequence, new_sequence};
pub use builtins::{DeferredClass, RepeatClass, SequenceClass};

#[cfg(test)]
mod tests;
