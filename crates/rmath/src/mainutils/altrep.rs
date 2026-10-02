#![cfg(feature = "altrep")]
#![allow(non_snake_case)]
//! Unsafe native callback adapter over the rooted Rust ALTREP implementation.
//! Native functions must uphold R's liveness, ownership and callback contracts.
//! Rust classes implement `sexp::altrep::AltrepClass` without raw pointer methods.

use crate::sexp::{
    accessors::*,
    altrep::{self, AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::{R_xlen_t, Rcomplex, SEXP, SEXPTYPE},
    object::{SexpError, SexpMut, SexpResult},
    owner::OwnerToken,
};
use std::{
    cell::RefCell,
    ffi::{CStr, c_char, c_int},
    rc::Rc,
};

/// GNU's class handle is a struct subtype, distinct from a vector SEXP.
///
/// The pointer is owner-local. Native callers must keep the originating session
/// alive; this layout alone does not imply complete GNU package ABI support.
#[allow(non_camel_case_types)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct R_altrep_class_t {
    pub ptr: SEXP,
}
impl From<SEXP> for R_altrep_class_t {
    fn from(ptr: SEXP) -> Self {
        Self { ptr }
    }
}

/// Callback used by GNU's Inspect method to inspect a child object.
pub type InspectSubtree = unsafe extern "C" fn(SEXP, c_int, c_int, c_int);
pub type InspectMethod =
    unsafe extern "C" fn(SEXP, c_int, c_int, c_int, Option<InspectSubtree>) -> c_int;

fn fail<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}
fn missing(what: &'static str) -> SexpError {
    SexpError::AllocationFailed { object: what }
}

/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_data1(x: SEXP) -> SEXP {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    fail(altrep::data1(&object)).as_raw()
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_data2(x: SEXP) -> SEXP {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    fail(altrep::data2(&object)).as_raw()
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_set_altrep_data1(x: SEXP, v: SEXP) {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let value = fail(unsafe { altrep::rooted_raw(v) });
    fail(altrep::set_data1(&object, value));
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_set_altrep_data2(x: SEXP, v: SEXP) {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let value = fail(unsafe { altrep::rooted_raw(v) });
    fail(altrep::set_data2(&object, value));
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_class(x: SEXP) -> SEXP {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    if let Some(class) = altrep::altrep_class(&object) {
        return class.as_raw();
    }
    if let Some(sequence) = object.compact_seq() {
        return unsafe {
            if sequence.is_int() {
                crate::mainutils::altclasses::R_init_compact_intseq()
            } else {
                crate::mainutils::altclasses::R_init_compact_realseq()
            }
        };
    }
    crate::sexp::context::r_error("object is not an ALTREP")
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_length(x: SEXP) -> R_xlen_t {
    if x.is_null() {
        0
    } else {
        unsafe { XLENGTH(x) }
    }
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_new_altrep(class: R_altrep_class_t, data1: SEXP, data2: SEXP) -> SEXP {
    let instance = crate::sexp::instance::current_instance_ptr().expect("active ALTREP owner");
    let owner = unsafe { OwnerToken::from_raw(instance) };
    let class = fail(altrep::class_handle(owner, class.ptr));
    fail(
        AltrepBuilder::new(class)
            .data1(fail(owner.sexp(data1)))
            .data2(fail(owner.sexp(data2)))
            .build(),
    )
    .as_raw()
}
/// Compatibility adapter for older Rust callers using a raw class descriptor.
/// # Safety
/// The descriptor and data must satisfy `R_new_altrep`'s ownership contract.
pub unsafe fn R_new_altrep_raw(class: SEXP, data1: SEXP, data2: SEXP) -> SEXP {
    unsafe { R_new_altrep(class.into(), data1, data2) }
}
/// # Safety
/// Both pointers must be live in the active owner; the class must originate
/// from that owner. No allocation or callback occurs for an ordinary vector.
pub unsafe fn R_altrep_inherits(x: SEXP, class: R_altrep_class_t) -> c_int {
    if x.is_null() || unsafe { ALTREP(x) } == 0 {
        return 0;
    }
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let descriptor = fail(unsafe { altrep::rooted_raw(class.ptr) });
    i32::from(unsafe { R_altrep_class(object.as_raw()) } == descriptor.as_raw())
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_compact_intseq(from: R_xlen_t, to: R_xlen_t) -> SEXP {
    let from = i32::try_from(from)
        .unwrap_or_else(|_| crate::sexp::context::r_error("integer sequence origin out of range"));
    let to = i32::try_from(to).unwrap_or_else(|_| {
        crate::sexp::context::r_error("integer sequence endpoint out of range")
    });
    let length = (i64::from(to) - i64::from(from)).unsigned_abs() + 1;
    let owner = unsafe {
        OwnerToken::from_raw(crate::sexp::instance::current_instance_ptr().expect("active owner"))
    };
    fail(altrep::new_sequence(
        owner,
        SEXPTYPE::INTSXP,
        from as f64,
        if to >= from { 1.0 } else { -1.0 },
        length as i64,
    ))
    .as_raw()
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_compact_realseq(from: f64, by: f64, length: R_xlen_t) -> SEXP {
    if length < 0 || length > (1_i64 << 52) {
        crate::sexp::context::r_error("invalid sequence length");
    }
    let owner = unsafe {
        OwnerToken::from_raw(crate::sexp::instance::current_instance_ptr().expect("active owner"))
    };
    fail(altrep::new_sequence(
        owner,
        SEXPTYPE::REALSXP,
        from,
        by,
        length,
    ))
    .as_raw()
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_realize(x: SEXP) -> SEXP {
    unsafe {
        DATAPTR(x);
    }
    x
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_duplicate(x: SEXP, deep: c_int) -> SEXP {
    if let Some(methods) = unsafe { native_methods(x) } {
        let table = methods.borrow();
        let is_legacy = table.duplicate_ex.is_none();
        let callback = table.duplicate_ex.or(table.duplicate);
        drop(table);
        if let Some(callback) = callback {
            let input = fail(unsafe { altrep::rooted_raw(x) });
            let result = fail(altrep::activate_for(&input, || unsafe {
                callback(input.clone().as_raw(), deep)
            }));
            if !result.is_null() {
                let output = fail(unsafe { altrep::rooted_raw(result) });
                if is_legacy && result != x {
                    fail(altrep::activate_for(&input, || unsafe {
                        crate::mainutils::duplicate::altrep_duplicate_attributes(result, x, deep);
                    }));
                }
                return output.as_raw();
            }
        }
    }
    // NULL requests the regular type-specific duplication path, avoiding recursion.
    std::ptr::null_mut()
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_inspect(x: SEXP, pre: c_int, deep: c_int) -> c_int {
    unsafe { ALTREP_INSPECT(x, pre, deep, 0, None) }
}
/// # Safety
/// The object must be live in the active owner and the subtree callback must
/// obey the same allocation, rooting and no-unwind contracts as class methods.
pub unsafe fn ALTREP_INSPECT(
    x: SEXP,
    pre: c_int,
    deep: c_int,
    pvec: c_int,
    inspect_subtree: Option<InspectSubtree>,
) -> c_int {
    let Some(methods) = (unsafe { native_methods(x) }) else {
        return 0;
    };
    let table = methods.borrow();
    let callback = table.inspect;
    let legacy = table.legacy_inspect;
    drop(table);
    let input = fail(unsafe { altrep::rooted_raw(x) });
    if let Some(callback) = callback {
        fail(altrep::activate_for(&input, || unsafe {
            callback(x, pre, deep, pvec, inspect_subtree)
        }))
    } else {
        legacy.map_or(0, |f| {
            fail(altrep::activate_for(&input, || unsafe { f(x, pre, deep) }))
        })
    }
}
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn R_altrep_coerce(x: SEXP, kind: c_int) -> SEXP {
    let Some(methods) = (unsafe { native_methods(x) }) else {
        return std::ptr::null_mut();
    };
    let callback = methods.borrow().coerce;
    let input = fail(unsafe { altrep::rooted_raw(x) });
    callback.map_or(std::ptr::null_mut(), |f| {
        let result = fail(altrep::activate_for(&input, || unsafe { f(x, kind) }));
        if result.is_null() {
            return result;
        }
        fail(unsafe { altrep::rooted_raw(result) }).as_raw()
    })
}

macro_rules! scalar_accessors {
    ($get:ident, $set:ident, $read:ident, $write:ident, $ty:ty, $absent:expr) => {
        pub unsafe fn $get(x: SEXP, i: R_xlen_t) -> $ty {
            if x.is_null() {
                return $absent;
            }
            match fail(unsafe { altrep::rooted_raw(x) }).$read(i) {
                Ok(value) => value,
                Err(SexpError::OutOfBounds { .. }) => $absent,
                Err(error) => fail(Err(error)),
            }
        }
        pub unsafe fn $set(x: SEXP, i: R_xlen_t, value: $ty) {
            let object = fail(unsafe { altrep::rooted_raw(x) });
            fail(fail(SexpMut::try_from_checked(object)).$write(i, value));
        }
    };
}
scalar_accessors!(
    ALTINTEGER_ELT,
    ALTINTEGER_SET_ELT,
    try_integer_elt,
    try_set_integer_elt,
    i32,
    crate::sexp::ffi::NA_INTEGER
);
scalar_accessors!(
    ALTREAL_ELT,
    ALTREAL_SET_ELT,
    try_real_elt,
    try_set_real_elt,
    f64,
    crate::sexp::ffi::NA_REAL
);
scalar_accessors!(
    ALTLOGICAL_ELT,
    ALTLOGICAL_SET_ELT,
    try_logical_elt,
    try_set_logical_elt,
    i32,
    crate::sexp::ffi::NA_LOGICAL
);
scalar_accessors!(
    ALTRAW_ELT,
    ALTRAW_SET_ELT,
    try_raw_elt,
    try_set_raw_elt,
    u8,
    0
);
scalar_accessors!(
    ALTCOMPLEX_ELT,
    ALTCOMPLEX_SET_ELT,
    try_complex_elt,
    try_set_complex_elt,
    Rcomplex,
    Rcomplex {
        r: crate::sexp::ffi::NA_REAL,
        i: crate::sexp::ffi::NA_REAL
    }
);
/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn ALTSTRING_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe { STRING_ELT(x, i) }
}

/// # Safety
/// Inputs must be live and rooted in the active owner. Exclude payload
/// loans during callbacks/materialization and retain returned objects during use.
pub unsafe fn ALTSTRING_SET_ELT(x: SEXP, i: R_xlen_t, v: SEXP) {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let value = fail(unsafe { altrep::rooted_raw(v) });
    fail(fail(SexpMut::try_from_checked(object)).try_set_string_elt(i, value));
}

#[derive(Clone, Default)]
pub(crate) struct NativeMethods {
    kind: Option<SEXPTYPE>,
    length: Option<unsafe extern "C" fn(SEXP) -> R_xlen_t>,
    duplicate: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
    duplicate_ex: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
    inspect: Option<InspectMethod>,
    legacy_inspect: Option<unsafe extern "C" fn(SEXP, c_int, c_int) -> c_int>,
    coerce: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
    integer: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> c_int>,
    real: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> f64>,
    logical: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> c_int>,
    raw: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> u8>,
    complex: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> Rcomplex>,
    string: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> SEXP>,
    list: Option<unsafe extern "C" fn(SEXP, R_xlen_t) -> SEXP>,
}
struct NativeClass {
    kind: SEXPTYPE,
    methods: Rc<RefCell<NativeMethods>>,
}
impl AltrepClass for NativeClass {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, context: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
        let callback = self
            .methods
            .borrow()
            .length
            .ok_or(missing("ALTREP Length method"))?;
        Ok(unsafe { callback(context.object().as_raw()) })
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        i: R_xlen_t,
    ) -> SexpResult<AltrepElement<'s>> {
        let methods = self.methods.borrow().clone();
        let object = context.object().as_raw();
        // Method-table borrow is over before any native reentry.
        unsafe {
            Ok(match self.kind {
                SEXPTYPE::INTSXP => AltrepElement::Integer(methods
                    .integer
                    .ok_or(missing("ALTREP integer Elt method"))?(
                    object, i
                )),
                SEXPTYPE::REALSXP => {
                    AltrepElement::Real(methods.real.ok_or(missing("ALTREP real Elt method"))?(
                        object, i,
                    ))
                }
                SEXPTYPE::LGLSXP => AltrepElement::Logical(methods
                    .logical
                    .ok_or(missing("ALTREP logical Elt method"))?(
                    object, i
                )),
                SEXPTYPE::RAWSXP => {
                    AltrepElement::Raw(methods.raw.ok_or(missing("ALTREP raw Elt method"))?(
                        object, i,
                    ))
                }
                SEXPTYPE::CPLXSXP => AltrepElement::Complex(methods
                    .complex
                    .ok_or(missing("ALTREP complex Elt method"))?(
                    object, i
                )),
                SEXPTYPE::STRSXP => AltrepElement::String(context.wrap(
                    methods.string.ok_or(missing("ALTREP string Elt method"))?(object, i),
                )?),
                SEXPTYPE::VECSXP => AltrepElement::List(context.wrap(
                    methods.list.ok_or(missing("ALTREP list Elt method"))?(object, i),
                )?),
                _ => return Err(missing("ALTREP vector type")),
            })
        }
    }
}
unsafe fn native_methods(x: SEXP) -> Option<Rc<RefCell<NativeMethods>>> {
    if !unsafe { altrep::has_extension_raw(x) } {
        return None;
    }
    let object = unsafe { altrep::rooted_raw(x) }.ok()?;
    altrep::native_methods(&object)
}
unsafe fn methods(class: R_altrep_class_t) -> Rc<RefCell<NativeMethods>> {
    let root = fail(unsafe { altrep::rooted_raw(class.ptr) });
    let current = crate::sexp::instance::current_instance_ptr().expect("active owner");
    let owner = unsafe { OwnerToken::from_raw(current) };
    let handle = fail(altrep::class_handle(owner, root.as_raw()));
    altrep::native_methods_for_class(&handle)
        .unwrap_or_else(|| crate::sexp::context::r_error("unregistered native ALTREP class"))
}
unsafe fn make_class(name: *const c_char, package: *const c_char, kind: SEXPTYPE) -> SEXP {
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let package = unsafe { CStr::from_ptr(package) }.to_string_lossy();
    let owner = unsafe {
        OwnerToken::from_raw(crate::sexp::instance::current_instance_ptr().expect("active owner"))
    };
    let methods = Rc::new(RefCell::new(NativeMethods {
        kind: Some(kind),
        ..NativeMethods::default()
    }));
    let handle = fail(altrep::register_native(
        owner,
        &format!("{package}::{name}"),
        Rc::new(NativeClass {
            kind,
            methods: methods.clone(),
        }),
        methods,
    ));
    handle.descriptor().as_raw()
}
macro_rules! class_constructor {
    ($name:ident, $kind:ident) => {
        /// # Safety
        /// Names must be valid C strings. Callbacks registered on the returned
        /// class must remain loaded and obey R ownership and reentry contracts.
        pub unsafe fn $name(
            name: *const c_char,
            package: *const c_char,
            _dll: *mut std::ffi::c_void,
        ) -> R_altrep_class_t {
            R_altrep_class_t {
                ptr: unsafe { make_class(name, package, SEXPTYPE::$kind) },
            }
        }
    };
}
class_constructor!(R_make_altinteger_class, INTSXP);
class_constructor!(R_make_altreal_class, REALSXP);
class_constructor!(R_make_altlogical_class, LGLSXP);
class_constructor!(R_make_altraw_class, RAWSXP);
class_constructor!(R_make_altcomplex_class, CPLXSXP);
class_constructor!(R_make_altstring_class, STRSXP);
class_constructor!(R_make_altlist_class, VECSXP);
macro_rules! method_setter {
    ($name:ident, $field:ident, $ty:ty, $kind:ident) => {
        /// # Safety
        /// The callback must obey the declared ABI, type, rooting and lifetime
        /// contracts. It cannot unwind through the C callback boundary.
        pub unsafe fn $name(class: R_altrep_class_t, callback: Option<$ty>) {
            let methods = unsafe { methods(class) };
            if methods.borrow().kind != Some(SEXPTYPE::$kind) {
                crate::sexp::context::r_error("ALTREP method does not match class vector type");
            }
            methods.borrow_mut().$field = callback;
        }
    };
    ($name:ident, $field:ident, $ty:ty) => {
        /// # Safety
        /// Callback code must outlive the session, root live objects during
        /// allocation, and uphold the declared type and length. No unwinding
        /// through a C ABI callback is permitted.
        pub unsafe fn $name(class: R_altrep_class_t, callback: Option<$ty>) {
            unsafe { methods(class) }.borrow_mut().$field = callback;
        }
    };
}
method_setter!(
    R_set_altrep_Length_method,
    length,
    unsafe extern "C" fn(SEXP) -> R_xlen_t
);
method_setter!(
    R_set_altrep_Duplicate_method,
    duplicate,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);
method_setter!(R_set_altrep_Inspect_method, inspect, InspectMethod);
method_setter!(
    R_set_altrep_Coerce_method,
    coerce,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);
method_setter!(
    R_set_altinteger_Elt_method,
    integer,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> c_int,
    INTSXP
);
method_setter!(
    R_set_altreal_Elt_method,
    real,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> f64,
    REALSXP
);
method_setter!(
    R_set_altlogical_Elt_method,
    logical,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> c_int,
    LGLSXP
);
method_setter!(
    R_set_altraw_Elt_method,
    raw,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> u8,
    RAWSXP
);
method_setter!(
    R_set_altcomplex_Elt_method,
    complex,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> Rcomplex,
    CPLXSXP
);
method_setter!(
    R_set_altstring_Elt_method,
    string,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> SEXP,
    STRSXP
);
method_setter!(
    R_set_altlist_Elt_method,
    list,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> SEXP,
    VECSXP
);

method_setter!(
    R_set_altrep_DuplicateEX_method,
    duplicate_ex,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);

// The previous Rust spelling remains available for downstream Rust users.
// Canonical methods above intentionally keep Rust error propagation until the
// runtime provides a C entry-point trampoline that can translate R errors.
macro_rules! legacy_setter {
    ($name:ident, $canonical:ident, $ty:ty) => {
        /// # Safety
        /// The class and callback must satisfy the canonical setter's contract.
        pub unsafe fn $name(class: impl Into<R_altrep_class_t>, callback: Option<$ty>) {
            unsafe { $canonical(class.into(), callback) }
        }
    };
}
legacy_setter!(
    R_set_altrep_length_method,
    R_set_altrep_Length_method,
    unsafe extern "C" fn(SEXP) -> R_xlen_t
);
legacy_setter!(
    R_set_altrep_duplicate_method,
    R_set_altrep_Duplicate_method,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);
legacy_setter!(
    R_set_altrep_coerce_method,
    R_set_altrep_Coerce_method,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);
/// Compatibility setter for the former three-argument Rust Inspect adapter.
/// # Safety
/// The class and callback must satisfy the canonical setter's contract.
pub unsafe fn R_set_altrep_inspect_method(
    class: impl Into<R_altrep_class_t>,
    callback: Option<unsafe extern "C" fn(SEXP, c_int, c_int) -> c_int>,
) {
    unsafe { methods(class.into()) }.borrow_mut().legacy_inspect = callback;
}

#[cfg(test)]
#[path = "altrep/compat_tests.rs"]
mod compat_tests;
