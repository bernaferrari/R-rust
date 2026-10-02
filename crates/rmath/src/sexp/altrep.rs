#![cfg(feature = "altrep")]
//! Rooted, typed ALTREP classes. Class code uses copied values and never loans
//! an R payload. The small raw adapter below owns allocation/publication; class
//! methods may allocate, collect or reenter R without a live arena borrow.
//!
//! Class descriptors are interned symbols. Instance data are ordinary GC-traced
//! VECSXP slots in an internal attribute, independent of the vector's type and
//! logical length. Rust method tables stay in the owning session.

use super::{
    ffi::{R_xlen_t, Rcomplex, SEXP, SEXPTYPE},
    object::{Sexp, SexpError, SexpMut, SexpResult},
    owner::OwnerToken,
    session::{RSession, with_instance_active},
};
use std::{collections::HashMap, ffi::CString, rc::Rc};

const TAG: &std::ffi::CStr = c".InternalAltrep";

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

/// Implementations cannot retain session objects: those belong in data1/data2.
/// A vector's type and length are fixed at construction. Element callbacks may
/// run R or collect; they return copied scalars or rooted child handles.
pub trait AltrepClass: 'static {
    fn vector_type(&self) -> SEXPTYPE;
    fn length(&self, context: &AltrepContext<'_>) -> SexpResult<R_xlen_t>;
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: R_xlen_t,
    ) -> SexpResult<AltrepElement<'s>>;
}

#[derive(Default)]
pub(crate) struct AltrepRuntimeState {
    classes: HashMap<usize, Rc<dyn AltrepClass>>,
    pub(crate) native:
        HashMap<usize, Rc<std::cell::RefCell<crate::mainutils::altrep::NativeMethods>>>,
    expanding: std::collections::HashSet<usize>,
    reading: std::collections::HashSet<(usize, i64)>,
}

/// Lifetime-bound activation and allocation for a class callback.
/// No method exposes a borrowed R buffer or a mutable RInstance reference.
pub struct AltrepContext<'s> {
    owner: OwnerToken<'s>,
    object: Sexp<'s>,
    data1: Sexp<'s>,
    data2: Sexp<'s>,
}
impl<'s> AltrepContext<'s> {
    pub(crate) fn wrap(&self, raw: SEXP) -> SexpResult<Sexp<'s>> {
        self.owner.sexp(raw)
    }
    pub fn object(&self) -> Sexp<'s> {
        self.object.clone()
    }
    pub fn data1(&self) -> Sexp<'s> {
        self.data1.clone()
    }
    pub fn data2(&self) -> Sexp<'s> {
        self.data2.clone()
    }
    pub fn set_data2(&self, value: Sexp<'s>) -> SexpResult<()> {
        let value = self.owner.sexp(value.as_raw())?;
        let data = metadata(&self.object).ok_or(failure("missing instance metadata"))?;
        SexpMut::try_from_checked(data)?.try_set_vector_elt(2, value)
    }
    pub fn gc(&self) -> SexpResult<()> {
        self.active(|| self.owner.full_gc().map(|_| ()))
    }
    pub fn alloc_vector(&self, kind: SEXPTYPE, length: R_xlen_t) -> SexpResult<Sexp<'s>> {
        allocate(self.owner, kind, length)
    }
    pub fn string(&self, text: &str) -> SexpResult<Sexp<'s>> {
        let ptr = self
            .active(|| unsafe { super::memory::with_arena(|a| a.alloc_charsxp(text.as_bytes())) });
        self.owner.sexp(ptr)
    }
    pub fn eval(&self, expression: Sexp<'s>, environment: Sexp<'s>) -> SexpResult<Sexp<'s>> {
        // Root and validate both inputs before entering translated code.
        let expression = self.owner.sexp(expression.as_raw())?;
        let environment = self.owner.sexp(environment.as_raw())?;
        let result = self
            .active(|| unsafe { crate::eval::eval::eval(expression.clone(), environment.clone()) });
        self.owner.sexp(
            result
                .map_err(|message| SexpError::EvaluationFailed { message })?
                .as_raw(),
        )
    }
    fn active<T>(&self, f: impl FnOnce() -> T) -> T {
        // SAFETY: the context's owner capability retains the session lifetime.
        unsafe { with_instance_active(self.owner.as_ptr(), f) }
    }
}

/// A session-bound class token. A descriptor cannot accidentally select a class
/// in a different session, or disappear while an instance is being built.
#[derive(Clone)]
pub struct AltrepClassHandle<'s> {
    owner: OwnerToken<'s>,
    descriptor: Sexp<'s>,
}
impl<'s> AltrepClassHandle<'s> {
    pub fn descriptor(&self) -> Sexp<'s> {
        self.descriptor.clone()
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

fn supported(kind: SEXPTYPE) -> bool {
    matches!(
        kind,
        SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::VECSXP
    )
}
fn failure(what: &'static str) -> SexpError {
    SexpError::Altrep { reason: what }
}

pub(crate) fn register<'s>(
    owner: OwnerToken<'s>,
    name: &str,
    class: Rc<dyn AltrepClass>,
) -> SexpResult<AltrepClassHandle<'s>> {
    if !supported(class.vector_type()) {
        return Err(failure("unsupported ALTREP vector type"));
    }
    let name = CString::new(format!(".AltrepClass.{name}"))
        .map_err(|_| failure("invalid ALTREP class name"))?;
    let raw = unsafe {
        with_instance_active(owner.as_ptr(), || super::symbol::Rf_install(name.as_ptr()))
    };
    let descriptor = owner.sexp(raw)?;
    // Registration is immutable: existing instances must retain their methods.
    // No callback or allocation of R objects runs while this field is borrowed.
    unsafe {
        let table = &mut (*owner.as_ptr()).altrep_state.classes;
        if table.contains_key(&(raw as usize)) {
            return Err(failure("ALTREP class already registered"));
        }
        table
            .try_reserve(1)
            .map_err(|_| failure("ALTREP class table"))?;
        table.insert(raw as usize, class);
    }
    Ok(AltrepClassHandle { owner, descriptor })
}

pub(crate) fn class_handle<'s>(
    owner: OwnerToken<'s>,
    raw: SEXP,
) -> SexpResult<AltrepClassHandle<'s>> {
    let descriptor = owner.sexp(raw)?;
    if lookup(owner, raw).is_none() {
        return Err(failure("unregistered ALTREP class"));
    }
    Ok(AltrepClassHandle { owner, descriptor })
}

/// Construct using two rooted data objects; builder order has no effect.
pub struct AltrepBuilder<'s> {
    class: AltrepClassHandle<'s>,
    data1: Sexp<'s>,
    data2: Sexp<'s>,
}
impl<'s> AltrepBuilder<'s> {
    pub fn new(class: AltrepClassHandle<'s>) -> Self {
        Self {
            class,
            data1: Sexp::nil(),
            data2: Sexp::nil(),
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
        let owner = self.class.owner;
        let data1 = owner.sexp(self.data1.as_raw())?;
        let data2 = owner.sexp(self.data2.as_raw())?;
        let class = lookup(owner, self.class.descriptor.clone().as_raw())
            .ok_or(failure("unregistered ALTREP class"))?;
        let kind = class.vector_type();
        let object = allocate(owner, kind, 0)?;
        let context = AltrepContext {
            owner,
            object: object.clone(),
            data1,
            data2,
        };

        let metadata = allocate(owner, SEXPTYPE::VECSXP, 3)?;
        let mut metadata = SexpMut::try_from_checked(metadata)?;
        metadata.try_set_vector_elt(0, self.class.descriptor)?;
        metadata.try_set_vector_elt(1, context.data1.clone())?;
        metadata.try_set_vector_elt(2, context.data2.clone())?;
        let metadata = metadata.freeze();
        let cell = context.active(|| unsafe {
            let tag = super::symbol::Rf_install(TAG.as_ptr());
            super::memory::with_arena(|a| {
                a.cons(metadata.clone().as_raw(), super::globals::R_NilValue(), tag)
            })
        });
        let cell = owner.sexp(cell)?;
        // Both nodes are rooted; publish only after the barrier succeeds.
        unsafe {
            if !super::gengc::write_barrier_in(
                owner.as_ptr(),
                object.clone().as_raw(),
                cell.clone().as_raw(),
            ) {
                return Err(failure("ALTREP metadata barrier"));
            }
            let raw = object.clone().as_raw();
            (*raw).attrib = cell.as_raw();
            (*raw).sxpinfo.set_alt(true);
        }
        let length = context.active(|| class.length(&context))?;
        if length < 0 || length > (1_i64 << 52) {
            return Err(failure("invalid ALTREP length"));
        }
        unsafe {
            (*object.clone().as_raw()).set_vecsxp_length(length);
        }
        Ok(object)
    }
}

fn allocate<'s>(owner: OwnerToken<'s>, kind: SEXPTYPE, length: R_xlen_t) -> SexpResult<Sexp<'s>> {
    if !supported(kind) {
        return Err(failure("unsupported vector type"));
    }
    usize::try_from(length).map_err(|_| failure("invalid vector length"))?;
    let raw = unsafe {
        with_instance_active(owner.as_ptr(), || {
            super::memory::with_arena(|a| a.alloc_vector(kind, length))
        })
    };
    owner.sexp(raw).map_err(|_| failure("ALTREP vector"))
}

fn lookup(owner: OwnerToken<'_>, descriptor: SEXP) -> Option<Rc<dyn AltrepClass>> {
    // Copy the Rc, ending the field borrow before calling any method.
    unsafe {
        (*owner.as_ptr())
            .altrep_state
            .classes
            .get(&(descriptor as usize))
            .cloned()
    }
}

fn owner<'s>(object: &Sexp<'s>) -> SexpResult<OwnerToken<'s>> {
    let pointer = object
        .session_owner_ptr
        .ok_or(SexpError::UncheckedMutation)?;
    // SAFETY: this rooted checked handle retains its original session.
    Ok(unsafe { OwnerToken::from_raw(pointer.as_ptr()) })
}

pub(crate) fn metadata<'s>(object: &Sexp<'s>) -> Option<Sexp<'s>> {
    if !object.header().sxpinfo.alt() {
        return None;
    }
    let cell = object.attrib()?;
    if cell.typeof_() != SEXPTYPE::LISTSXP {
        return None;
    }
    let tag = cell.tag()?;
    let name = tag.header();
    let super::object::NodeBody::Symbol(symbol) = name.body else {
        return None;
    };
    if !tag.copied_header(symbol.pname)?.char_eq(TAG.to_bytes()) {
        return None;
    }
    let data = cell.car()?;
    (data.typeof_() == SEXPTYPE::VECSXP && data.len() == 3).then_some(data)
}

fn context<'s>(object: &Sexp<'s>) -> SexpResult<(AltrepContext<'s>, Rc<dyn AltrepClass>)> {
    let owner = owner(object)?;
    let data = metadata(object).ok_or(failure("invalid ALTREP metadata"))?;
    let descriptor = data.try_vector_elt(0)?;
    let class = lookup(owner, descriptor.as_raw()).ok_or(failure("unregistered ALTREP class"))?;
    if class.vector_type() != object.typeof_() {
        return Err(failure("ALTREP class type mismatch"));
    }
    Ok((
        AltrepContext {
            owner,
            object: object.clone(),
            data1: data.try_vector_elt(1)?,
            data2: data.try_vector_elt(2)?,
        },
        class,
    ))
}

pub fn is_altrep(object: &Sexp<'_>) -> bool {
    object.header().sxpinfo.alt()
}
pub fn is_materialized(object: &Sexp<'_>) -> bool {
    !object.header().payload.is_null() || object.is_empty()
}
pub fn altrep_class<'s>(object: &Sexp<'s>) -> Option<Sexp<'s>> {
    metadata(object)?.vector_elt(0)
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
    if is_materialized(object) || metadata(object).is_none() {
        return dense_element(object, index);
    }
    let (context, class) = context(object)?;
    let value = invoke_element(&context, &*class, index)?;
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
struct Reading<'s> {
    owner: OwnerToken<'s>,
    key: (usize, i64),
}
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        unsafe {
            (*self.owner.as_ptr())
                .altrep_state
                .reading
                .remove(&self.key);
        }
    }
}
fn invoke_element<'s>(
    context: &AltrepContext<'s>,
    class: &dyn AltrepClass,
    index: i64,
) -> SexpResult<AltrepElement<'s>> {
    let key = (context.object.clone().as_raw() as usize, index);
    unsafe {
        let reading = &mut (*context.owner.as_ptr()).altrep_state.reading;
        reading
            .try_reserve(1)
            .map_err(|_| failure("element callback state allocation"))?;
        if !reading.insert(key) {
            return Err(failure("recursive element callback"));
        }
    }
    let _reading = Reading {
        owner: context.owner,
        key,
    };
    context.active(|| class.element(context, index))
}
pub(crate) fn lazy_element<'s>(
    object: &Sexp<'s>,
    index: R_xlen_t,
) -> Option<SexpResult<AltrepElement<'s>>> {
    if !object.header().payload.is_null() || metadata(object).is_none() {
        return None;
    }
    Some(altrep_elt(object, index))
}
/// # Safety
/// Raw translated callers retain the active owner and exclude payload loans.
pub(crate) unsafe fn lazy_raw<'s>(
    raw: SEXP,
    index: R_xlen_t,
) -> Option<SexpResult<AltrepElement<'s>>> {
    let object = unsafe { rooted_raw(raw) }.ok()?;
    lazy_element(&object, index)
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

struct Expansion<'s> {
    owner: OwnerToken<'s>,
    key: usize,
}
impl Drop for Expansion<'_> {
    fn drop(&mut self) {
        unsafe {
            (*self.owner.as_ptr())
                .altrep_state
                .expanding
                .remove(&self.key);
        }
    }
}
/// Transactional expansion. Failure leaves the original lazy values intact.
/// Collection can trace the private output and metadata during each callback.
pub fn force_materialization(object: &Sexp<'_>) -> SexpResult<()> {
    if metadata(object).is_none() || is_materialized(object) {
        return Ok(());
    }
    let (context, class) = context(object)?;
    let key = object.clone().as_raw() as usize;
    unsafe {
        let active = &mut (*context.owner.as_ptr()).altrep_state.expanding;
        active
            .try_reserve(1)
            .map_err(|_| failure("ALTREP expansion state"))?;
        if !active.insert(key) {
            return Err(failure("recursive ALTREP materialization"));
        }
    }
    let _expansion = Expansion {
        owner: context.owner,
        key,
    };
    if super::memory::is_arena_lent(context.owner.as_ptr()) {
        return Err(failure("release the arena lend before materialization"));
    }
    let output = allocate(context.owner, object.typeof_(), object.len())?;
    let mut output = SexpMut::try_from_checked(output)?;
    for i in 0..object.len() {
        write_element(
            &mut output,
            i,
            validate_element(&context, invoke_element(&context, &*class, i)?)?,
        )?;
    }
    let output = output.freeze();
    // No callback or payload loan survives. Both vectors have the same owner,
    // type and length. Ownership of the tracked buffer moves to the target;
    // the scratch node becomes an empty vector and cannot free that buffer.
    unsafe {
        let target = object.clone().as_raw();
        let source = output.clone().as_raw();
        if !super::gengc::write_barrier_in(context.owner.as_ptr(), target, source) {
            return Err(failure("ALTREP payload barrier"));
        }
        (*target).gengc_next_node = (*source).gengc_next_node;
        (*target).set_vecsxp_truelength(object.len());
        (*source).gengc_next_node = std::ptr::null_mut();
        (*source).set_vecsxp_length(0);
        (*source).set_vecsxp_truelength(0);
    }
    Ok(())
}

/// Raw runtime adapter: validate ownership and retain a lease before callbacks.
/// # Safety
/// `raw` must name a live node in the active session; no payload loan may overlap.
pub(crate) unsafe fn rooted_raw<'a>(raw: SEXP) -> SexpResult<Sexp<'a>> {
    let current = super::instance::current_instance_ptr().ok_or(SexpError::OwnerNotActive)?;
    // This unsafe boundary relies on the caller retaining the owner lifetime.
    unsafe { OwnerToken::from_raw(current) }.sexp(raw)
}

/// Called by raw and checked bulk accessors. Returns false for compact sequences.
/// # Safety
/// Same owner and loan requirements as `rooted_raw`.
pub(crate) unsafe fn materialize_raw(raw: SEXP) -> SexpResult<bool> {
    let object = unsafe { rooted_raw(raw) }?;
    if metadata(&object).is_none() {
        return Ok(false);
    }
    force_materialization(&object)?;
    Ok(true)
}

/// Create a dense value copy, preserving public attributes. Serialization uses
/// this fallback when a class does not have a portable serialized state.
pub fn materialized_copy<'s>(object: &Sexp<'s>) -> SexpResult<Sexp<'s>> {
    let owner = owner(object)?;
    let output = allocate(owner, object.typeof_(), object.len())?;
    let mut output = SexpMut::try_from_checked(output)?;
    for i in 0..object.len() {
        write_element(&mut output, i, altrep_elt(object, i)?)?;
    }
    let output = output.freeze();
    let attributes = if metadata(object).is_some() {
        object.attrib().and_then(|cell| cell.cdr())
    } else {
        object.attrib()
    };
    if let Some(attributes) = attributes {
        unsafe {
            if !super::gengc::write_barrier_in(
                owner.as_ptr(),
                output.clone().as_raw(),
                attributes.clone().as_raw(),
            ) {
                return Err(failure("copy attributes barrier"));
            }
            let raw = output.clone().as_raw();
            (*raw).attrib = attributes.as_raw();
            (*raw).sxpinfo.set_obj(object.header().sxpinfo.obj());
            (*raw).sxpinfo.set_gp(object.header().sxpinfo.gp());
        }
    }
    Ok(output)
}

pub(crate) fn builtin_sequence<'s>(
    owner: OwnerToken<'s>,
    kind: SEXPTYPE,
) -> SexpResult<AltrepClassHandle<'s>> {
    let name = match kind {
        SEXPTYPE::INTSXP => ".builtin.compact_intseq",
        SEXPTYPE::REALSXP => ".builtin.compact_realseq",
        _ => return Err(failure("sequence vector type")),
    };
    let symbol = CString::new(format!(".AltrepClass.{name}")).unwrap();
    let raw = unsafe {
        with_instance_active(owner.as_ptr(), || {
            super::symbol::Rf_install(symbol.as_ptr())
        })
    };
    if lookup(owner, raw).is_some() {
        return class_handle(owner, raw);
    }
    register(owner, name, Rc::new(SequenceClass(kind)))
}

pub(crate) fn new_sequence<'s>(
    owner: OwnerToken<'s>,
    kind: SEXPTYPE,
    origin: f64,
    step: f64,
    length: i64,
) -> SexpResult<Sexp<'s>> {
    let class = builtin_sequence(owner, kind)?;
    let state = allocate(owner, SEXPTYPE::REALSXP, 3)?;
    let mut state = SexpMut::try_from_checked(state)?;
    state.try_set_real_elt(0, length as f64)?;
    state.try_set_real_elt(1, origin)?;
    state.try_set_real_elt(2, step)?;
    AltrepBuilder::new(class).data1(state.freeze()).build()
}

/// Formula in data1: GNU's `[length, origin, step]` real triple.
/// Both classes retain that state after expanding, independently of values.
pub struct SequenceClass(pub SEXPTYPE);
impl AltrepClass for SequenceClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<i64> {
        let state = c.data1();
        if state.len() != 3 {
            return Err(failure("sequence state must contain three scalars"));
        }
        let length = state.try_real_elt(0)?;
        if !length.is_finite()
            || length < 0.0
            || length > (1_u64 << 52) as f64
            || length.fract() != 0.0
        {
            return Err(failure("invalid sequence length"));
        }
        if self.0 == SEXPTYPE::INTSXP {
            let first = state.try_real_elt(1)?;
            let step = state.try_real_elt(2)?;
            let last = first + (length - 1.0).max(0.0) * step;
            if [first, step, last].iter().any(|v| {
                !v.is_finite() || v.fract() != 0.0 || *v < i32::MIN as f64 || *v > i32::MAX as f64
            }) {
                return Err(failure("integer sequence out of range"));
            }
        }
        Ok(length as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        let state = c.data1();
        let value = state.try_real_elt(1)? + i as f64 * state.try_real_elt(2)?;
        match self.0 {
            SEXPTYPE::INTSXP
                if value.is_finite()
                    && value.fract() == 0.0
                    && value >= i32::MIN as f64
                    && value <= i32::MAX as f64 =>
            {
                Ok(AltrepElement::Integer(value as i32))
            }
            SEXPTYPE::REALSXP => Ok(AltrepElement::Real(value)),
            _ => Err(failure("invalid sequence element")),
        }
    }
}

/// Deferred evaluation with a traced result cache. data1 is a list containing
/// `[expression, environment, length]`; data2 starts at NULL. Evaluation failure
/// leaves the cache empty, so retry is possible. Cached results must have the
/// declared vector type and length.
pub struct DeferredClass(pub SEXPTYPE);
impl AltrepClass for DeferredClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<i64> {
        let len = c.data1().try_vector_elt(2)?.try_real_elt(0)?;
        if !len.is_finite() || len < 0.0 || len > (1_u64 << 52) as f64 || len.fract() != 0.0 {
            return Err(failure("invalid deferred vector length"));
        }
        Ok(len as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        let cached = c.data2();
        let cached = if cached.typeof_() == SEXPTYPE::NILSXP {
            let data = c.data1();
            let result = c.eval(data.try_vector_elt(0)?, data.try_vector_elt(1)?)?;
            if result.typeof_() != self.0 || result.len() != c.object().len() {
                return Err(failure("deferred result type or length mismatch"));
            }
            c.set_data2(result.clone())?;
            result
        } else {
            cached
        };
        if cached.typeof_() != self.0 || cached.len() != c.object().len() {
            return Err(failure("invalid deferred result cache"));
        }
        dense_element(&cached, i)
    }
}

impl RSession {
    /// Safe built-in compact vectors using the production formula representation.
    pub fn compact_integer_sequence(
        &self,
        origin: i32,
        step: i32,
        length: usize,
    ) -> SexpResult<Sexp<'_>> {
        if length > 0 {
            let last = i128::from(origin) + (length - 1) as i128 * i128::from(step);
            if last < i32::MIN as i128 || last > i32::MAX as i128 {
                return Err(failure("integer sequence out of range"));
            }
        }
        let owner = self.owner_token().ok_or(SexpError::OwnerNotActive)?;
        let raw = unsafe {
            with_instance_active(owner.as_ptr(), || {
                super::altseq::compact_int_seq(origin, step, length)
            })
        };
        owner.sexp(raw)
    }
    pub fn compact_real_sequence(
        &self,
        origin: f64,
        step: f64,
        length: usize,
    ) -> SexpResult<Sexp<'_>> {
        let owner = self.owner_token().ok_or(SexpError::OwnerNotActive)?;
        let raw = unsafe {
            with_instance_active(owner.as_ptr(), || {
                super::altseq::compact_real_seq(origin, step, length)
            })
        };
        owner.sexp(raw)
    }
}

/// Rust built-in repeated atomic or list elements. Data1 is the scalar source,
/// data2 an integer/real length. GC traces both independently of the payload.
pub struct RepeatClass(pub SEXPTYPE);
impl AltrepClass for RepeatClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.0
    }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
        if c.data1.len() != 1 || c.data1.typeof_() != self.0 {
            return Err(failure("ALTREP repeat scalar"));
        }
        let len = c.data2.try_real_elt(0)?;
        if !len.is_finite() || len < 0.0 || len.fract() != 0.0 || len >= i64::MAX as f64 {
            return Err(failure("ALTREP repeat length"));
        }
        Ok(len as i64)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, _: R_xlen_t) -> SexpResult<AltrepElement<'s>> {
        dense_element(&c.data1, 0)
    }
}

#[cfg(test)]
mod tests;
