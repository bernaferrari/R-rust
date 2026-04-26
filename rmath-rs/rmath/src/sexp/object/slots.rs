use std::os::raw::c_void;

use super::{Sexp, SexpError, SexpResult};
use crate::sexp::ffi::{R_xlen_t, Rcomplex, SEXP, SEXPTYPE};

impl<'a> Sexp<'a> {
    // --- Closure accessors ---

    /// Get the formal parameters of a closure.
    ///
    /// Returns `None` if this is not a closure or the formals are null.
    #[inline]
    pub fn formals(self) -> Option<Sexp<'a>> {
        if self.is_closure() {
            Sexp::from_raw(unsafe { (*self.ptr).data.closxp.formals })
        } else {
            None
        }
    }

    /// Get the formal parameters of a closure with typed error reporting.
    #[inline]
    pub fn try_formals(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::CLOSXP, "closure")?;
        Self::checked_child(unsafe { (*self.ptr).data.closxp.formals })
    }

    /// Get the body of a closure.
    ///
    /// Returns `None` if this is not a closure or the body is null.
    #[inline]
    pub fn body(self) -> Option<Sexp<'a>> {
        if self.is_closure() {
            Sexp::from_raw(unsafe { (*self.ptr).data.closxp.body })
        } else {
            None
        }
    }

    /// Get the body of a closure with typed error reporting.
    #[inline]
    pub fn try_body(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::CLOSXP, "closure")?;
        Self::checked_child(unsafe { (*self.ptr).data.closxp.body })
    }

    /// Get the environment of a closure.
    ///
    /// Returns `None` if this is not a closure or the environment is null.
    #[inline]
    pub fn cloenv(self) -> Option<Sexp<'a>> {
        if self.is_closure() {
            Sexp::from_raw(unsafe { (*self.ptr).data.closxp.env })
        } else {
            None
        }
    }

    /// Get the environment of a closure with typed error reporting.
    #[inline]
    pub fn try_cloenv(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::CLOSXP, "closure")?;
        Self::checked_child(unsafe { (*self.ptr).data.closxp.env })
    }

    // --- Environment accessors ---

    /// Get the frame of an environment.
    ///
    /// Returns `None` if this is not an environment or the frame is null.
    #[inline]
    pub fn frame(self) -> Option<Sexp<'a>> {
        if self.is_environment() {
            Sexp::from_raw(unsafe { (*self.ptr).data.envsxp.frame })
        } else {
            None
        }
    }

    /// Get the frame of an environment with typed error reporting.
    #[inline]
    pub fn try_frame(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::ENVSXP, "environment")?;
        Self::checked_child(unsafe { (*self.ptr).data.envsxp.frame })
    }

    /// Get the enclosing (parent) environment.
    ///
    /// Returns `None` if this is not an environment or the enclosing env is null.
    #[inline]
    pub fn enclos(self) -> Option<Sexp<'a>> {
        if self.is_environment() {
            Sexp::from_raw(unsafe { (*self.ptr).data.envsxp.enclos })
        } else {
            None
        }
    }

    /// Get the enclosing environment with typed error reporting.
    #[inline]
    pub fn try_enclos(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::ENVSXP, "environment")?;
        Self::checked_child(unsafe { (*self.ptr).data.envsxp.enclos })
    }

    /// Get the hash table of an environment.
    ///
    /// Returns `None` if this is not an environment or the hashtab is null.
    #[inline]
    pub fn hashtab(self) -> Option<Sexp<'a>> {
        if self.is_environment() {
            Sexp::from_raw(unsafe { (*self.ptr).data.envsxp.hashtab })
        } else {
            None
        }
    }

    /// Get the hash table of an environment with typed error reporting.
    #[inline]
    pub fn try_hashtab(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::ENVSXP, "environment")?;
        Self::checked_child(unsafe { (*self.ptr).data.envsxp.hashtab })
    }

    // --- Promise accessors ---

    /// Get the value of a promise.
    ///
    /// Returns `None` if this is not a promise or the value is null.
    #[inline]
    pub fn prvalue(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::PROMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.promsxp.value })
        } else {
            None
        }
    }

    /// Get the value of a promise with typed error reporting.
    #[inline]
    pub fn try_prvalue(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::PROMSXP, "promise")?;
        Self::checked_child(unsafe { (*self.ptr).data.promsxp.value })
    }

    /// Get the code/expression of a promise.
    ///
    /// Returns `None` if this is not a promise or the code is null.
    #[inline]
    pub fn prcode(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::PROMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.promsxp.expr })
        } else {
            None
        }
    }

    /// Get the code/expression of a promise with typed error reporting.
    #[inline]
    pub fn try_prcode(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::PROMSXP, "promise")?;
        Self::checked_child(unsafe { (*self.ptr).data.promsxp.expr })
    }

    /// Get the environment of a promise.
    ///
    /// Returns `None` if this is not a promise or the environment is null.
    #[inline]
    pub fn prenv(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::PROMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.promsxp.env })
        } else {
            None
        }
    }

    /// Get the environment of a promise with typed error reporting.
    #[inline]
    pub fn try_prenv(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::PROMSXP, "promise")?;
        Self::checked_child(unsafe { (*self.ptr).data.promsxp.env })
    }

    // --- Symbol accessors ---

    /// Get the value of a symbol binding.
    ///
    /// Returns `None` if this is not a symbol or the value is null.
    #[inline]
    pub fn symvalue(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::SYMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.symsxp.internal })
        } else {
            None
        }
    }

    /// Get the value of a symbol binding with typed error reporting.
    #[inline]
    pub fn try_symvalue(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::SYMSXP, "symbol")?;
        Self::checked_child(unsafe { (*self.ptr).data.symsxp.internal })
    }

    /// Get the print name of a symbol.
    ///
    /// Returns `None` if this is not a symbol or the print name is null.
    #[inline]
    pub fn printname(self) -> Option<Sexp<'a>> {
        if self.typeof_() == SEXPTYPE::SYMSXP {
            Sexp::from_raw(unsafe { (*self.ptr).data.symsxp.pname })
        } else {
            None
        }
    }

    /// Get the print name of a symbol with typed error reporting.
    #[inline]
    pub fn try_printname(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::SYMSXP, "symbol")?;
        Self::checked_child(unsafe { (*self.ptr).data.symsxp.pname })
    }

    // --- Attribute access ---

    /// Get the attributes of this SEXP.
    ///
    /// Returns `None` if there are no attributes.
    #[inline]
    pub fn attrib(self) -> Option<Sexp<'a>> {
        Sexp::from_raw(unsafe { (*self.ptr).attrib })
    }

    /// Get the attributes of this SEXP, returning `NULL` when there are none.
    #[inline]
    pub fn try_attrib(self) -> SexpResult<Sexp<'a>> {
        Self::checked_child(unsafe { (*self.ptr).attrib })
    }

    /// Check if this object has the OBJECT flag set (has a class attribute).
    ///
    /// S3 and S4 objects have this flag set, triggering method dispatch.
    #[inline]
    pub fn is_object(self) -> bool {
        unsafe { (*self.ptr).sxpinfo.obj() }
    }

    // --- CHARSXP accessors ---

    #[inline]
    pub fn is_charsxp(self) -> bool {
        self.typeof_() == SEXPTYPE::CHARSXP
    }

    pub fn char_len(self) -> Option<R_xlen_t> {
        if self.is_charsxp() {
            Some(unsafe { (*self.ptr).data.charsxp_truelen })
        } else {
            None
        }
    }

    /// Return the CHARSXP byte length with typed error reporting.
    pub fn try_char_len(self) -> SexpResult<R_xlen_t> {
        self.expect_type(SEXPTYPE::CHARSXP, "character scalar")?;
        Ok(unsafe { (*self.ptr).data.charsxp_truelen })
    }

    pub fn as_bytes(self) -> Option<&'a [u8]> {
        self.try_as_bytes().ok()
    }

    /// Return the CHARSXP bytes with typed error reporting.
    pub fn try_as_bytes(self) -> SexpResult<&'a [u8]> {
        self.expect_type(SEXPTYPE::CHARSXP, "character scalar")?;
        let len = unsafe { (*self.ptr).data.charsxp_truelen } as usize;
        let data = unsafe { (*self.ptr).gengc_next_node as *const u8 };
        if len == 0 {
            return Ok(&[]);
        }
        if data.is_null() {
            return Err(SexpError::MissingData {
                sexptype: SEXPTYPE::CHARSXP,
            });
        }
        Ok(unsafe { std::slice::from_raw_parts(data, len) })
    }

    pub fn as_str(self) -> Option<&'a str> {
        self.try_as_str().ok()
    }

    /// Return the CHARSXP bytes as UTF-8 with typed error reporting.
    pub fn try_as_str(self) -> SexpResult<&'a str> {
        std::str::from_utf8(self.try_as_bytes()?).map_err(|_| SexpError::InvalidUtf8)
    }

    // --- Complex vector accessors ---

    pub fn set_complex_elt(self, i: R_xlen_t, v: Rcomplex) -> bool {
        self.try_set_complex_elt(i, v).is_ok()
    }

    /// Set the i-th complex value with typed error reporting.
    pub fn try_set_complex_elt(self, i: R_xlen_t, v: Rcomplex) -> SexpResult<()> {
        let data = self.try_typed_data_mut::<Rcomplex>(SEXPTYPE::CPLXSXP, "complex vector")?;
        let i = self.try_index(i)?;
        unsafe { *data.add(i) = v };
        Ok(())
    }

    pub fn as_complex_slice(self) -> Option<&'a [Rcomplex]> {
        self.try_as_complex_slice().ok()
    }

    /// Get a complex slice view with typed error reporting.
    pub fn try_as_complex_slice(self) -> SexpResult<&'a [Rcomplex]> {
        self.try_typed_slice::<Rcomplex>(SEXPTYPE::CPLXSXP, "complex vector")
    }

    pub fn iter_complex(self) -> impl Iterator<Item = Rcomplex> + 'a {
        self.as_complex_slice().unwrap_or(&[]).iter().copied()
    }

    // --- Dot-dot-dot (DOTSXP) ---

    #[inline]
    pub fn is_dots(self) -> bool {
        self.typeof_() == SEXPTYPE::DOTSXP
    }

    // --- Bytecode (BCODESXP) ---

    #[inline]
    pub fn is_bytecode(self) -> bool {
        self.typeof_() == SEXPTYPE::BCODESXP
    }

    // --- External pointer (EXTPTRSXP) ---

    #[inline]
    pub fn is_extptr(self) -> bool {
        self.typeof_() == SEXPTYPE::EXTPTRSXP
    }

    pub fn extptr_ptr(self) -> Option<*mut c_void> {
        if self.is_extptr() {
            Some(unsafe { (*self.ptr).data.extptr[0] })
        } else {
            None
        }
    }

    /// Get the external pointer payload with typed error reporting.
    ///
    /// A null external pointer payload is a valid R value and is returned as-is.
    pub fn try_extptr_ptr(self) -> SexpResult<*mut c_void> {
        self.expect_type(SEXPTYPE::EXTPTRSXP, "external pointer")?;
        Ok(unsafe { (*self.ptr).data.extptr[0] })
    }

    pub fn extptr_tag(self) -> Option<Sexp<'a>> {
        if self.is_extptr() {
            Sexp::from_raw(unsafe { (*self.ptr).data.extptr[1] as SEXP })
        } else {
            None
        }
    }

    /// Get the external pointer tag with typed error reporting.
    pub fn try_extptr_tag(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::EXTPTRSXP, "external pointer")?;
        Self::checked_child(unsafe { (*self.ptr).data.extptr[1] as SEXP })
    }

    pub fn extprot(self) -> Option<Sexp<'a>> {
        if self.is_extptr() {
            Sexp::from_raw(unsafe { (*self.ptr).data.extptr[2] as SEXP })
        } else {
            None
        }
    }

    /// Get the external pointer protected value with typed error reporting.
    pub fn try_extprot(self) -> SexpResult<Sexp<'a>> {
        self.expect_type(SEXPTYPE::EXTPTRSXP, "external pointer")?;
        Self::checked_child(unsafe { (*self.ptr).data.extptr[2] as SEXP })
    }

    // --- Weak reference (WEAKREFSXP) ---

    #[inline]
    pub fn is_weakref(self) -> bool {
        self.typeof_() == SEXPTYPE::WEAKREFSXP
    }

    // --- S4 object (OBJSXP) ---

    #[inline]
    pub fn is_s4(self) -> bool {
        self.typeof_() == SEXPTYPE::OBJSXP
    }

    // --- Expression vector (EXPRSXP) ---

    #[inline]
    pub fn is_expression(self) -> bool {
        self.typeof_() == SEXPTYPE::EXPRSXP
    }

    // --- Function (FUNSXP) ---

    #[inline]
    pub fn is_function(self) -> bool {
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
    #[inline]
    pub fn try_data_ptr(self) -> SexpResult<*mut c_void> {
        if self.typeof_().is_vector_type() || self.typeof_() == SEXPTYPE::CHARSXP {
            let ptr = unsafe { (*self.ptr).gengc_next_node as *mut c_void };
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
