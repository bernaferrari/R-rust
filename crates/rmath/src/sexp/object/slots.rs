use std::os::raw::c_void;

use super::header::NodeBody;
use super::{Sexp, SexpError, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rcomplex, SEXP, SEXPTYPE};

#[allow(deprecated)] // deprecated Sexp set_* shims delegate to try_set_* shims
impl<'a> Sexp<'a> {
    // --- Closure accessors ---

    /// Get the formal parameters of a closure.
    ///
    /// Returns `None` if this is not a closure or the formals are null.
    #[inline]
    pub fn formals(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Closure(slot) => self.optional_child(slot.formals),
            _ => None,
        }
    }

    /// Get the formal parameters of a closure with typed error reporting.
    #[inline]
    pub fn try_formals(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Closure(slot) => self.checked_child(slot.formals),
            _ => self.closure_mismatch(),
        }
    }

    /// Get the body of a closure.
    ///
    /// Returns `None` if this is not a closure or the body is null.
    #[inline]
    pub fn body(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Closure(slot) => self.optional_child(slot.body),
            _ => None,
        }
    }

    /// Get the body of a closure with typed error reporting.
    #[inline]
    pub fn try_body(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Closure(slot) => self.checked_child(slot.body),
            _ => self.closure_mismatch(),
        }
    }

    /// Get the environment of a closure.
    ///
    /// Returns `None` if this is not a closure or the environment is null.
    #[inline]
    pub fn cloenv(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Closure(slot) => self.optional_child(slot.env),
            _ => None,
        }
    }

    /// Get the environment of a closure with typed error reporting.
    #[inline]
    pub fn try_cloenv(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Closure(slot) => self.checked_child(slot.env),
            _ => self.closure_mismatch(),
        }
    }

    fn closure_mismatch(&self) -> SexpResult<Sexp<'a>> {
        Err(SexpError::TypeMismatch {
            expected: "closure",
            actual: self.typeof_(),
        })
    }

    // --- Environment accessors ---

    /// Get the frame of an environment.
    ///
    /// Returns `None` if this is not an environment or the frame is null.
    #[inline]
    pub fn frame(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Environment(slot) => self.optional_child(slot.frame),
            _ => None,
        }
    }

    /// Get the frame of an environment with typed error reporting.
    #[inline]
    pub fn try_frame(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Environment(slot) => self.checked_child(slot.frame),
            _ => self.environment_mismatch(),
        }
    }

    /// Get the enclosing (parent) environment.
    ///
    /// Returns `None` if this is not an environment or the enclosing env is null.
    #[inline]
    pub fn enclos(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Environment(slot) => self.optional_child(slot.enclos),
            _ => None,
        }
    }

    /// Get the enclosing environment with typed error reporting.
    #[inline]
    pub fn try_enclos(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Environment(slot) => self.checked_child(slot.enclos),
            _ => self.environment_mismatch(),
        }
    }

    /// Get the hash table of an environment.
    ///
    /// Returns `None` if this is not an environment or the hashtab is null.
    #[inline]
    pub fn hashtab(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Environment(slot) => self.optional_child(slot.hashtab),
            _ => None,
        }
    }

    /// Get the hash table of an environment with typed error reporting.
    #[inline]
    pub fn try_hashtab(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Environment(slot) => self.checked_child(slot.hashtab),
            _ => self.environment_mismatch(),
        }
    }

    fn environment_mismatch(&self) -> SexpResult<Sexp<'a>> {
        Err(SexpError::TypeMismatch {
            expected: "environment",
            actual: self.typeof_(),
        })
    }

    // --- Promise accessors ---

    /// Get the value of a promise.
    ///
    /// Returns `None` if this is not a promise or the value is null.
    #[inline]
    pub fn prvalue(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Promise(slot) => self.optional_child(slot.value),
            _ => None,
        }
    }

    /// Get the value of a promise with typed error reporting.
    #[inline]
    pub fn try_prvalue(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Promise(slot) => self.checked_child(slot.value),
            _ => self.promise_mismatch(),
        }
    }

    /// Get the code/expression of a promise.
    ///
    /// Returns `None` if this is not a promise or the code is null.
    #[inline]
    pub fn prcode(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Promise(slot) => self.optional_child(slot.expr),
            _ => None,
        }
    }

    /// Get the code/expression of a promise with typed error reporting.
    #[inline]
    pub fn try_prcode(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Promise(slot) => self.checked_child(slot.expr),
            _ => self.promise_mismatch(),
        }
    }

    /// Get the environment of a promise.
    ///
    /// Returns `None` if this is not a promise or the environment is null.
    #[inline]
    pub fn prenv(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Promise(slot) => self.optional_child(slot.env),
            _ => None,
        }
    }

    /// Get the environment of a promise with typed error reporting.
    #[inline]
    pub fn try_prenv(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Promise(slot) => self.checked_child(slot.env),
            _ => self.promise_mismatch(),
        }
    }

    fn promise_mismatch(&self) -> SexpResult<Sexp<'a>> {
        Err(SexpError::TypeMismatch {
            expected: "promise",
            actual: self.typeof_(),
        })
    }

    // --- Symbol accessors ---

    /// Get the value of a symbol binding.
    ///
    /// Returns `None` if this is not a symbol or the value is null.
    #[inline]
    pub fn symvalue(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Symbol(slot) => self.optional_child(slot.internal),
            _ => None,
        }
    }

    /// Get the value of a symbol binding with typed error reporting.
    #[inline]
    pub fn try_symvalue(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Symbol(slot) => self.checked_child(slot.internal),
            _ => self.symbol_mismatch(),
        }
    }

    /// Get the print name of a symbol.
    ///
    /// Returns `None` if this is not a symbol or the print name is null.
    #[inline]
    pub fn printname(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::Symbol(slot) => self.optional_child(slot.pname),
            _ => None,
        }
    }

    /// Get the print name of a symbol with typed error reporting.
    #[inline]
    pub fn try_printname(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::Symbol(slot) => self.checked_child(slot.pname),
            _ => self.symbol_mismatch(),
        }
    }

    fn symbol_mismatch(&self) -> SexpResult<Sexp<'a>> {
        Err(SexpError::TypeMismatch {
            expected: "symbol",
            actual: self.typeof_(),
        })
    }

    // --- Attribute access ---

    /// Get the attributes of this SEXP.
    ///
    /// Returns `None` if there are no attributes.
    #[inline]
    pub fn attrib(&self) -> Option<Sexp<'a>> {
        self.optional_child(self.header().attrib)
    }

    /// Get the attributes of this SEXP, returning `NULL` when there are none.
    #[inline]
    pub fn try_attrib(&self) -> SexpResult<Sexp<'a>> {
        self.checked_child(self.header().attrib)
    }

    /// Check if this object has the OBJECT flag set (has a class attribute).
    ///
    /// S3 and S4 objects have this flag set, triggering method dispatch.
    #[inline]
    pub fn is_object(&self) -> bool {
        self.header().sxpinfo.obj()
    }

    // --- CHARSXP accessors ---

    #[inline]
    pub fn is_charsxp(&self) -> bool {
        self.typeof_() == SEXPTYPE::CHARSXP
    }

    pub fn char_len(&self) -> Option<R_xlen_t> {
        let header = self.header();
        if header.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
            return None;
        }
        match header.body {
            NodeBody::Vector(vec) => Some(vec.length),
            _ => None,
        }
    }

    /// Return the CHARSXP byte length with typed error reporting.
    pub fn try_char_len(&self) -> SexpResult<R_xlen_t> {
        self.char_len().ok_or_else(|| SexpError::TypeMismatch {
            expected: "character scalar",
            actual: self.typeof_(),
        })
    }

    /// Compare CHARSXP bytes without retaining a borrow of the payload.
    pub(crate) fn try_char_eq(&self, expected: &[u8]) -> SexpResult<bool> {
        let header = self.header();
        if header.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
            return Err(SexpError::TypeMismatch {
                expected: "character scalar",
                actual: header.sxpinfo.type_of(),
            });
        }
        let NodeBody::Vector(vec) = header.body else {
            return Err(SexpError::TypeMismatch {
                expected: "character scalar",
                actual: header.sxpinfo.type_of(),
            });
        };
        if vec.length == 0 {
            return Ok(expected.is_empty());
        }
        if vec.length < 0 || header.payload.is_null() {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        }
        Ok(header.char_eq(expected))
    }

    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_bytes(&self) -> Option<&'_ [u8]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_bytes()
        }
        .ok()
    }

    /// Return the CHARSXP bytes with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_bytes(&self) -> SexpResult<&'_ [u8]> {
        let header = self.header();
        if header.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
            return Err(SexpError::TypeMismatch {
                expected: "character scalar",
                actual: header.sxpinfo.type_of(),
            });
        }
        let NodeBody::Vector(vec) = header.body else {
            return Err(SexpError::TypeMismatch {
                expected: "character scalar",
                actual: header.sxpinfo.type_of(),
            });
        };
        if vec.length == 0 {
            return Ok(&[]);
        }
        let Ok(len) = usize::try_from(vec.length) else {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        };
        let data = header.payload as *const u8;
        if data.is_null() {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        }
        // SAFETY: caller retains the handle and excludes payload mutation for
        // the returned borrow. The length is the CHARSXP byte count.
        Ok(unsafe { std::slice::from_raw_parts(data, len) })
    }

    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_str(&self) -> Option<&'_ str> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_str()
        }
        .ok()
    }

    /// Return the CHARSXP bytes as UTF-8 with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_str(&self) -> SexpResult<&'_ str> {
        std::str::from_utf8(unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_bytes()
        }?).map_err(|_| SexpError::InvalidUtf8)
    }

    /// Copy a character scalar into owned UTF-8 text.
    pub fn try_as_string(&self) -> SexpResult<String> {
        // SAFETY: copying invokes no R code, and self retains its allocation.
        unsafe { self.try_as_str() }.map(str::to_owned)
    }

    pub fn as_string(&self) -> Option<String> {
        self.try_as_string().ok()
    }

    // --- Complex vector accessors ---

    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn set_complex_elt(self, i: R_xlen_t, v: Rcomplex) -> bool {
        unsafe {
            /* SAFETY: caller excludes borrowed payload views. */
            self.try_set_complex_elt(i, v)
        }
        .is_ok()
    }

    /// Set the i-th complex value with typed error reporting.
    /// # Safety
    /// The object must remain live and have no borrowed payload references
    /// during this write. Consuming a clone does not prove exclusivity.
    #[doc(hidden)]
    #[deprecated(
        note = "translation-compat shim: mutate through SexpMut::from_owned(..), then freeze()"
    )]
    pub(crate) unsafe fn try_set_complex_elt(self, i: R_xlen_t, v: Rcomplex) -> SexpResult<()> {
        let data = self
            .clone()
            .try_typed_data_mut::<Rcomplex>(SEXPTYPE::CPLXSXP, "complex vector")
            .clone()?;
        let i = self.try_index(i)?;
        unsafe { *data.add(i) = v };
        Ok(())
    }

    #[allow(clippy::wrong_self_convention)]
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn as_complex_slice(&self) -> Option<&'_ [Rcomplex]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_as_complex_slice()
        }
        .ok()
    }

    /// Get a complex slice view with typed error reporting.
    /// # Safety
    /// Retain this handle and exclude all mutation of the borrowed payload
    /// until the returned reference dies. Do not execute R while it is borrowed.
    pub(super) unsafe fn try_as_complex_slice(&self) -> SexpResult<&'_ [Rcomplex]> {
        unsafe {
            /* SAFETY: caller retains the handle and excludes payload mutation. */
            self.try_typed_slice::<Rcomplex>(SEXPTYPE::CPLXSXP, "complex vector")
        }
    }

    pub fn iter_complex(self) -> impl Iterator<Item = Rcomplex> + 'a {
        (0..self.len()).filter_map(move |i| self.complex_elt(i))
    }

    // --- Dot-dot-dot (DOTSXP) ---

    #[inline]
    pub fn is_dots(&self) -> bool {
        self.typeof_() == SEXPTYPE::DOTSXP
    }

    // --- Bytecode (BCODESXP) ---

    #[inline]
    pub fn is_bytecode(&self) -> bool {
        self.typeof_() == SEXPTYPE::BCODESXP
    }

    // --- External pointer (EXTPTRSXP) ---

    #[inline]
    pub fn is_extptr(&self) -> bool {
        self.typeof_() == SEXPTYPE::EXTPTRSXP
    }

    pub fn extptr_ptr(&self) -> Option<*mut c_void> {
        match self.header().body {
            NodeBody::ExtPtr(slot) => Some(slot[0]),
            _ => None,
        }
    }

    /// Get the external pointer payload with typed error reporting.
    ///
    /// A null external pointer payload is a valid R value and is returned as-is.
    pub fn try_extptr_ptr(&self) -> SexpResult<*mut c_void> {
        match self.header().body {
            NodeBody::ExtPtr(slot) => Ok(slot[0]),
            _ => Err(SexpError::TypeMismatch {
                expected: "external pointer",
                actual: self.typeof_(),
            }),
        }
    }

    pub fn extptr_tag(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::ExtPtr(slot) => self.optional_child(slot[1] as SEXP),
            _ => None,
        }
    }

    /// Get the external pointer tag with typed error reporting.
    pub fn try_extptr_tag(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::ExtPtr(slot) => self.checked_child(slot[1] as SEXP),
            _ => Err(SexpError::TypeMismatch {
                expected: "external pointer",
                actual: self.typeof_(),
            }),
        }
    }

    pub fn extprot(&self) -> Option<Sexp<'a>> {
        match self.header().body {
            NodeBody::ExtPtr(slot) => self.optional_child(slot[2] as SEXP),
            _ => None,
        }
    }

    /// Get the external pointer protected value with typed error reporting.
    pub fn try_extprot(&self) -> SexpResult<Sexp<'a>> {
        match self.header().body {
            NodeBody::ExtPtr(slot) => self.checked_child(slot[2] as SEXP),
            _ => Err(SexpError::TypeMismatch {
                expected: "external pointer",
                actual: self.typeof_(),
            }),
        }
    }

    // --- Weak reference (WEAKREFSXP) ---

    #[inline]
    pub fn is_weakref(&self) -> bool {
        self.typeof_() == SEXPTYPE::WEAKREFSXP
    }

    // --- S4 object (OBJSXP) ---

    #[inline]
    pub fn is_s4(&self) -> bool {
        self.typeof_() == SEXPTYPE::OBJSXP
    }

    // --- Expression vector (EXPRSXP) ---

    #[inline]
    pub fn is_expression(&self) -> bool {
        self.typeof_() == SEXPTYPE::EXPRSXP
    }

    // --- Function (FUNSXP) ---

    #[inline]
    pub fn is_function(&self) -> bool {
        let t = self.typeof_();
        t == SEXPTYPE::CLOSXP || t == SEXPTYPE::SPECIALSXP || t == SEXPTYPE::BUILTINSXP
    }

    // --- Data pointer ---

    /// Get the raw data pointer for vector types.
    ///
    /// Returns `None` for non-vector types or if the data pointer is null.
    /// The returned pointer points to the element data buffer (same as
    /// R's `DATAPTR()`).
    #[inline]
    pub fn data_ptr(self) -> Option<*mut c_void> {
        self.try_data_ptr().ok()
    }

    /// Get the raw data pointer for vector-like objects with typed errors.
    ///
    /// A compact sequence is expanded first, matching `DATAPTR`.
    #[inline]
    pub fn try_data_ptr(self) -> SexpResult<*mut c_void> {
        if self.typeof_().is_vector_type() || self.typeof_() == SEXPTYPE::CHARSXP {
            self.materialize_compact_payload()?;
            let ptr = self.header().payload as *mut c_void;
            if ptr.is_null() {
                Err(SexpError::MissingData {
                    sexptype: self.typeof_(),
                })
            } else {
                Ok(ptr)
            }
        } else {
            Err(SexpError::TypeMismatch {
                expected: "vector or character scalar",
                actual: self.typeof_(),
            })
        }
    }
}
