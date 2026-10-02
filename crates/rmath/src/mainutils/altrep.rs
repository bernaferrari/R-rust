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

fn fail<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::sexp::context::r_error(&e.to_string()))
}
fn missing(what: &'static str) -> SexpError {
    SexpError::AllocationFailed { object: what }
}

pub unsafe fn R_altrep_data1(x: SEXP) -> SEXP {
    unsafe { data(x, 1) }
}
pub unsafe fn R_altrep_data2(x: SEXP) -> SEXP {
    unsafe { data(x, 2) }
}
unsafe fn data(x: SEXP, i: i64) -> SEXP {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let metadata = altrep::metadata(&object).unwrap_or_else(|| {
        crate::sexp::context::r_error("object has no extension ALTREP metadata")
    });
    fail(metadata.try_vector_elt(i)).as_raw()
}
pub unsafe fn R_set_altrep_data1(x: SEXP, v: SEXP) {
    unsafe { set_data(x, v, 1) }
}
pub unsafe fn R_set_altrep_data2(x: SEXP, v: SEXP) {
    unsafe { set_data(x, v, 2) }
}
unsafe fn set_data(x: SEXP, v: SEXP, i: i64) {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let metadata = altrep::metadata(&object).unwrap_or_else(|| {
        crate::sexp::context::r_error("object has no extension ALTREP metadata")
    });
    let child = fail(unsafe { altrep::rooted_raw(v) });
    fail(fail(SexpMut::try_from_checked(metadata)).try_set_vector_elt(i, child));
}
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
pub unsafe fn R_altrep_length(x: SEXP) -> R_xlen_t {
    if x.is_null() {
        0
    } else {
        unsafe { XLENGTH(x) }
    }
}
pub unsafe fn R_new_altrep(class: SEXP, data1: SEXP, data2: SEXP) -> SEXP {
    let instance = crate::sexp::instance::current_instance_ptr().expect("active ALTREP owner");
    let owner = unsafe { OwnerToken::from_raw(instance) };
    let class = fail(altrep::class_handle(owner, class));
    fail(
        AltrepBuilder::new(class)
            .data1(fail(owner.sexp(data1)))
            .data2(fail(owner.sexp(data2)))
            .build(),
    )
    .as_raw()
}
pub unsafe fn R_compact_intseq(from: R_xlen_t, to: R_xlen_t) -> SEXP {
    let from = i32::try_from(from)
        .unwrap_or_else(|_| crate::sexp::context::r_error("integer sequence origin out of range"));
    let to = i32::try_from(to).unwrap_or_else(|_| {
        crate::sexp::context::r_error("integer sequence endpoint out of range")
    });
    let length = usize::try_from((i64::from(to) - i64::from(from)).unsigned_abs() + 1)
        .unwrap_or_else(|_| crate::sexp::context::r_error("sequence length out of range"));
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
pub unsafe fn R_altrep_realize(x: SEXP) -> SEXP {
    unsafe {
        DATAPTR(x);
    }
    x
}
pub unsafe fn R_altrep_duplicate(x: SEXP, deep: c_int) -> SEXP {
    if let Some(methods) = unsafe { native_methods(x) } {
        let callback = methods.borrow().duplicate;
        if let Some(callback) = callback {
            let input = fail(unsafe { altrep::rooted_raw(x) });
            let result = unsafe { callback(input.clone().as_raw(), deep) };
            if !result.is_null() {
                return fail(unsafe { altrep::rooted_raw(result) }).as_raw();
            }
        }
    }
    // NULL requests the regular type-specific duplication path, avoiding recursion.
    std::ptr::null_mut()
}
pub unsafe fn R_altrep_inspect(x: SEXP, pre: c_int, deep: c_int) -> c_int {
    let Some(methods) = (unsafe { native_methods(x) }) else {
        return 0;
    };
    let callback = methods.borrow().inspect;
    let _root = fail(unsafe { altrep::rooted_raw(x) });
    callback.map_or(0, |f| unsafe { f(x, pre, deep) })
}
pub unsafe fn R_altrep_coerce(x: SEXP, kind: c_int) -> SEXP {
    let Some(methods) = (unsafe { native_methods(x) }) else {
        return std::ptr::null_mut();
    };
    let callback = methods.borrow().coerce;
    let _root = fail(unsafe { altrep::rooted_raw(x) });
    callback.map_or(std::ptr::null_mut(), |f| {
        fail(unsafe { altrep::rooted_raw(f(x, kind)) }).as_raw()
    })
}

macro_rules! scalar_accessors {
    ($get:ident, $set:ident, $read:ident, $write:ident, $ty:ty, $absent:expr) => {
        pub unsafe fn $get(x: SEXP, i: R_xlen_t) -> $ty {
            if x.is_null() {
                return $absent;
            }
            fail(unsafe { altrep::rooted_raw(x) })
                .$read(i)
                .unwrap_or($absent)
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
    integer_elt,
    try_set_integer_elt,
    i32,
    crate::sexp::ffi::NA_INTEGER
);
scalar_accessors!(
    ALTREAL_ELT,
    ALTREAL_SET_ELT,
    real_elt,
    try_set_real_elt,
    f64,
    crate::sexp::ffi::NA_REAL
);
scalar_accessors!(
    ALTLOGICAL_ELT,
    ALTLOGICAL_SET_ELT,
    logical_elt,
    try_set_logical_elt,
    i32,
    crate::sexp::ffi::NA_LOGICAL
);
scalar_accessors!(ALTRAW_ELT, ALTRAW_SET_ELT, raw_elt, try_set_raw_elt, u8, 0);
scalar_accessors!(
    ALTCOMPLEX_ELT,
    ALTCOMPLEX_SET_ELT,
    complex_elt,
    try_set_complex_elt,
    Rcomplex,
    Rcomplex {
        r: crate::sexp::ffi::NA_REAL,
        i: crate::sexp::ffi::NA_REAL
    }
);
pub unsafe fn ALTSTRING_ELT(x: SEXP, i: R_xlen_t) -> SEXP {
    fail(fail(unsafe { altrep::rooted_raw(x) }).try_string_elt(i)).as_raw()
}
pub unsafe fn ALTSTRING_SET_ELT(x: SEXP, i: R_xlen_t, v: SEXP) {
    let object = fail(unsafe { altrep::rooted_raw(x) });
    let value = fail(unsafe { altrep::rooted_raw(v) });
    fail(fail(SexpMut::try_from_checked(object)).try_set_string_elt(i, value));
}

#[derive(Clone, Default)]
pub(crate) struct NativeMethods {
    length: Option<unsafe extern "C" fn(SEXP) -> R_xlen_t>,
    duplicate: Option<unsafe extern "C" fn(SEXP, c_int) -> SEXP>,
    inspect: Option<unsafe extern "C" fn(SEXP, c_int, c_int) -> c_int>,
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
    let object = unsafe { altrep::rooted_raw(x) }.ok()?;
    let class = altrep::altrep_class(&object)?.as_raw();
    let current = crate::sexp::instance::current_instance_ptr()?;
    unsafe {
        (*current)
            .altrep_state
            .native
            .get(&(class as usize))
            .cloned()
    }
}
unsafe fn methods(class: SEXP) -> Rc<RefCell<NativeMethods>> {
    let _root = fail(unsafe { altrep::rooted_raw(class) });
    let current = crate::sexp::instance::current_instance_ptr().expect("active owner");
    unsafe {
        (*current)
            .altrep_state
            .native
            .get(&(class as usize))
            .cloned()
    }
    .unwrap_or_else(|| crate::sexp::context::r_error("unregistered native ALTREP class"))
}
unsafe fn make_class(name: *const c_char, package: *const c_char, kind: SEXPTYPE) -> SEXP {
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let package = unsafe { CStr::from_ptr(package) }.to_string_lossy();
    let owner = unsafe {
        OwnerToken::from_raw(crate::sexp::instance::current_instance_ptr().expect("active owner"))
    };
    let methods = Rc::new(RefCell::new(NativeMethods::default()));
    let handle = fail(altrep::register(
        owner,
        &format!("{package}::{name}"),
        Rc::new(NativeClass {
            kind,
            methods: methods.clone(),
        }),
    ));
    let descriptor = handle.descriptor().as_raw();
    unsafe {
        (*owner.as_ptr())
            .altrep_state
            .native
            .insert(descriptor as usize, methods);
    }
    descriptor
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
        ) -> SEXP {
            unsafe { make_class(name, package, SEXPTYPE::$kind) }
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
    ($name:ident, $field:ident, $ty:ty) => {
        /// # Safety
        /// Callback code must outlive the session, root live objects during
        /// allocation, and uphold the declared type and length. No unwinding
        /// through a C ABI callback is permitted.
        pub unsafe fn $name(class: SEXP, callback: Option<$ty>) {
            unsafe { methods(class) }.borrow_mut().$field = callback;
        }
    };
}
method_setter!(
    R_set_altrep_length_method,
    length,
    unsafe extern "C" fn(SEXP) -> R_xlen_t
);
method_setter!(
    R_set_altrep_duplicate_method,
    duplicate,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);
method_setter!(
    R_set_altrep_inspect_method,
    inspect,
    unsafe extern "C" fn(SEXP, c_int, c_int) -> c_int
);
method_setter!(
    R_set_altrep_coerce_method,
    coerce,
    unsafe extern "C" fn(SEXP, c_int) -> SEXP
);
method_setter!(
    R_set_altinteger_Elt_method,
    integer,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> c_int
);
method_setter!(
    R_set_altreal_Elt_method,
    real,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> f64
);
method_setter!(
    R_set_altlogical_Elt_method,
    logical,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> c_int
);
method_setter!(
    R_set_altraw_Elt_method,
    raw,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> u8
);
method_setter!(
    R_set_altcomplex_Elt_method,
    complex,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> Rcomplex
);
method_setter!(
    R_set_altstring_Elt_method,
    string,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> SEXP
);
method_setter!(
    R_set_altlist_Elt_method,
    list,
    unsafe extern "C" fn(SEXP, R_xlen_t) -> SEXP
);
