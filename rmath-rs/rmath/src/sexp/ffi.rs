#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! FFI-compatible raw type definitions matching R's C memory layout.
//!
//! These types use `#[repr(C)]` and are designed to be ABI-compatible
//! with R's SEXPREC structures. Centralizes types that were previously
//! duplicated across 8+ files in mainutils/.

use std::os::raw::{c_char, c_double, c_int, c_void};

// ---------------------------------------------------------------------------
// Primitive type aliases (centralized from duplicates)
// ---------------------------------------------------------------------------

/// R's NA_INTEGER sentinel value.
pub const NA_INTEGER: c_int = c_int::MIN;

/// R's NA_LOGICAL sentinel value.
pub const NA_LOGICAL: c_int = c_int::MIN;

/// R's NA_REAL bit pattern (IEEE 754 quiet NaN with specific payload).
pub const R_NA_BIT_PATTERN: u64 = 0x7ff0000000001954;

/// R's NA_REAL sentinel.
pub const NA_REAL: c_double = f64::from_bits(0x7FF80000000007A2);

/// R's boolean type (0 = FALSE, 1 = TRUE, NA_LOGICAL = NA).
pub type Rboolean = c_int;

/// R's raw byte type.
pub type Rbyte = u8;

/// R's unsigned size type.
pub type R_size_t = usize;

/// R's extended length type (64-bit signed).
pub type R_xlen_t = i64;

/// R's length type (32-bit signed, used for most APIs).
pub type R_len_t = c_int;

/// R's TRUE constant.
pub const TRUE: c_int = 1;

/// R's FALSE constant.
pub const FALSE: c_int = 0;

/// DOTSXP type alias — same value as SEXPTYPE::DOTSXP.
pub const DOTSXP: c_int = 17;

// ---------------------------------------------------------------------------
// Rcomplex
// ---------------------------------------------------------------------------

/// R's complex number struct, matching C's Rcomplex layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rcomplex {
    pub r: c_double,
    pub i: c_double,
}

// ---------------------------------------------------------------------------
// SEXPTYPE
// ---------------------------------------------------------------------------

/// R's SEXPTYPE -- the type tag for all R objects.
///
/// Values must match R's C definitions exactly for ABI compatibility.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SEXPTYPE(pub c_int);

impl SEXPTYPE {
    pub const NILSXP: SEXPTYPE = SEXPTYPE(0);
    pub const SYMSXP: SEXPTYPE = SEXPTYPE(1);
    pub const LISTSXP: SEXPTYPE = SEXPTYPE(2);
    pub const CLOSXP: SEXPTYPE = SEXPTYPE(3);
    pub const ENVSXP: SEXPTYPE = SEXPTYPE(4);
    pub const PROMSXP: SEXPTYPE = SEXPTYPE(5);
    pub const LANGSXP: SEXPTYPE = SEXPTYPE(6);
    pub const SPECIALSXP: SEXPTYPE = SEXPTYPE(7);
    pub const BUILTINSXP: SEXPTYPE = SEXPTYPE(8);
    pub const CHARSXP: SEXPTYPE = SEXPTYPE(9);
    pub const LGLSXP: SEXPTYPE = SEXPTYPE(10);
    pub const INTSXP: SEXPTYPE = SEXPTYPE(13);
    pub const REALSXP: SEXPTYPE = SEXPTYPE(14);
    pub const CPLXSXP: SEXPTYPE = SEXPTYPE(15);
    pub const STRSXP: SEXPTYPE = SEXPTYPE(16);
    pub const DOTSXP: SEXPTYPE = SEXPTYPE(17);
    pub const ANYSXP: SEXPTYPE = SEXPTYPE(18);
    pub const VECSXP: SEXPTYPE = SEXPTYPE(19);
    pub const EXPRSXP: SEXPTYPE = SEXPTYPE(20);
    pub const BCODESXP: SEXPTYPE = SEXPTYPE(21);
    pub const EXTPTRSXP: SEXPTYPE = SEXPTYPE(22);
    pub const WEAKREFSXP: SEXPTYPE = SEXPTYPE(23);
    pub const RAWSXP: SEXPTYPE = SEXPTYPE(24);
    pub const OBJSXP: SEXPTYPE = SEXPTYPE(25);
    pub const FUNSXP: SEXPTYPE = SEXPTYPE(99);

    /// Check if this type is a vector type (has length/trueLength fields).
    #[inline]
    pub fn is_vector_type(self) -> bool {
        matches!(self.0, 10 | 13 | 14 | 15 | 16 | 19 | 20 | 24)
    }

    /// Check if this type is a list-like type (has CAR/CDR/TAG fields).
    #[inline]
    pub fn is_list_type(self) -> bool {
        self.0 == 2 || self.0 == 6 // LISTSXP, LANGSXP
    }

    /// Check if this is an atomic vector type.
    #[inline]
    pub fn is_atomic_type(self) -> bool {
        matches!(self.0, 10 | 13 | 14 | 15 | 16 | 24) // LGL, INT, REAL, CPLX, STR, RAW
    }
}

// ---------------------------------------------------------------------------
// SxpInfo -- header bit fields
// ---------------------------------------------------------------------------

/// Header information for every R object.
///
/// In C this is packed into 32 bits via bit-fields:
///   type(5) | scalar(1) | obj(1) | alt(1) | gp(16) |
///   mark(1) | debug(1) | trace(1) | spare(1) | gcgen(1) | gccls(3) | named(2)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxpInfo {
    /// Packed type info and flags (32 bits).
    pub type_and_flags: u32,
    /// Reference count (0..7, 0 means >7).
    pub rcount: u8,
    /// Padding.
    pub _pad: u8,
    pub _pad2: u16,
}

impl SxpInfo {
    /// Create a new SxpInfo with the given type.
    pub fn new(sexptype: SEXPTYPE) -> Self {
        SxpInfo {
            type_and_flags: sexptype.0 as u32 & 0x1F,
            rcount: 0,
            _pad: 0,
            _pad2: 0,
        }
    }

    // --- Getters ---

    #[inline]
    pub fn type_of(&self) -> SEXPTYPE {
        SEXPTYPE((self.type_and_flags & 0x1F) as c_int)
    }

    #[inline]
    pub fn scalar(&self) -> bool {
        (self.type_and_flags & (1 << 5)) != 0
    }

    #[inline]
    pub fn obj(&self) -> bool {
        (self.type_and_flags & (1 << 6)) != 0
    }

    #[inline]
    pub fn alt(&self) -> bool {
        (self.type_and_flags & (1 << 7)) != 0
    }

    #[inline]
    pub fn gp(&self) -> u16 {
        ((self.type_and_flags >> 8) & 0xFFFF) as u16
    }

    #[inline]
    pub fn mark(&self) -> bool {
        (self.type_and_flags & (1 << 24)) != 0
    }

    #[inline]
    pub fn debug(&self) -> bool {
        (self.type_and_flags & (1 << 25)) != 0
    }

    #[inline]
    pub fn trace(&self) -> bool {
        (self.type_and_flags & (1 << 26)) != 0
    }

    #[inline]
    pub fn spare(&self) -> bool {
        (self.type_and_flags & (1 << 27)) != 0
    }

    #[inline]
    pub fn gcgen(&self) -> u8 {
        ((self.type_and_flags >> 28) & 0x01) as u8
    }

    #[inline]
    pub fn gccls(&self) -> u8 {
        ((self.type_and_flags >> 29) & 0x07) as u8
    }

    /// Namedness level (0, 1, or 2).
    #[inline]
    pub fn named(&self) -> u8 {
        ((self.type_and_flags >> 29) & 0x03) as u8
    }

    // --- Setters ---

    #[inline]
    pub fn set_type(&mut self, t: SEXPTYPE) {
        self.type_and_flags = (self.type_and_flags & !0x1F) | (t.0 as u32 & 0x1F);
    }

    #[inline]
    pub fn set_scalar(&mut self, v: bool) {
        self.type_and_flags = (self.type_and_flags & !(1 << 5)) | ((v as u32) << 5);
    }

    #[inline]
    pub fn set_obj(&mut self, v: bool) {
        self.type_and_flags = (self.type_and_flags & !(1 << 6)) | ((v as u32) << 6);
    }

    #[inline]
    pub fn set_alt(&mut self, v: bool) {
        self.type_and_flags = (self.type_and_flags & !(1 << 7)) | ((v as u32) << 7);
    }

    #[inline]
    pub fn set_gp(&mut self, g: u16) {
        self.type_and_flags = (self.type_and_flags & !(0xFFFF << 8)) | ((g as u32) << 8);
    }

    #[inline]
    pub fn set_mark(&mut self, v: bool) {
        self.type_and_flags = (self.type_and_flags & !(1 << 24)) | ((v as u32) << 24);
    }

    #[inline]
    pub fn set_named(&mut self, n: u8) {
        self.type_and_flags = (self.type_and_flags & !(0x03 << 29)) | ((n as u32 & 0x03) << 29);
    }

    #[inline]
    pub fn set_gcgen(&mut self, v: u8) {
        self.type_and_flags = (self.type_and_flags & !(1 << 28)) | ((v as u32 & 0x01) << 28);
    }
}

// ---------------------------------------------------------------------------
// Union member structs (type-specific data)
// ---------------------------------------------------------------------------

/// Primitive function data (offset into function table).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Primsxp {
    pub offset: c_int,
}

/// Symbol data.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Symsxp {
    pub pname: *mut SexprecCore,
    pub value: *mut SexprecCore,
    pub internal: *mut SexprecCore,
}

/// List/cons cell data (LISTSXP and LANGSXP).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Listsxp {
    pub carval: *mut SexprecCore,
    pub cdrval: *mut SexprecCore,
    pub tagval: *mut SexprecCore,
}

/// Environment data.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Envsxp {
    pub frame: *mut SexprecCore,
    pub enclos: *mut SexprecCore,
    pub hashtab: *mut SexprecCore,
}

/// Closure data.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Closxp {
    pub formals: *mut SexprecCore,
    pub body: *mut SexprecCore,
    pub env: *mut SexprecCore,
}

/// Promise data.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Promsxp {
    pub value: *mut SexprecCore,
    pub expr: *mut SexprecCore,
    pub env: *mut SexprecCore,
}

/// Vector data header (length and true length).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Vecsxp {
    pub length: R_xlen_t,
    pub truelength: R_xlen_t,
}

// ---------------------------------------------------------------------------
// SexprecData union
// ---------------------------------------------------------------------------

/// Union storage for type-specific data.
///
/// Uses the largest variant for sizing. The actual interpretation
/// depends on the SEXPTYPE in the SxpInfo header.
#[repr(C)]
#[derive(Clone, Copy)]
pub union SexprecData {
    pub primsxp: Primsxp,
    pub symsxp: Symsxp,
    pub listsxp: Listsxp,
    pub envsxp: Envsxp,
    pub closxp: Closxp,
    pub promsxp: Promsxp,
    pub vecsxp: Vecsxp,
    /// For CHARSXP: offset to inline char data or length.
    pub charsxp_truelen: R_xlen_t,
    /// For EXTPTRSXP: pointer, tag, and protection info.
    pub extptr: [*mut c_void; 3],
}

impl Default for SexprecData {
    fn default() -> Self {
        SexprecData {
            vecsxp: Vecsxp::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The core SEXPREC structure -- the fundamental R object.
///
/// This is the unified scalar/vector node. For scalar types (SYMSXP,
/// LISTSXP, CLOSXP, etc.), the `data` union holds type-specific pointers.
/// For vector types, `data.vecsxp` holds length/truelength, and the
/// actual element data is stored in a separate allocation referenced
/// by the arena allocator.
#[repr(C)]
pub struct SexprecCore {
    pub sxpinfo: SxpInfo,
    pub attrib: *mut SexprecCore,
    pub gengc_next_node: *mut SexprecCore,
    pub gengc_prev_node: *mut SexprecCore,
    pub data: SexprecData,
}

impl SexprecCore {
    /// Create a new SexprecCore with the given type.
    pub fn new(sexptype: SEXPTYPE) -> Self {
        SexprecCore {
            sxpinfo: SxpInfo::new(sexptype),
            attrib: std::ptr::null_mut(),
            gengc_next_node: std::ptr::null_mut(),
            gengc_prev_node: std::ptr::null_mut(),
            data: SexprecData::default(),
        }
    }

    /// Create a new vector SexprecCore with length.
    pub fn new_vector(sexptype: SEXPTYPE, length: R_xlen_t) -> Self {
        let mut node = Self::new(sexptype);
        node.data = SexprecData {
            vecsxp: Vecsxp {
                length,
                truelength: length,
            },
        };
        node
    }
}

/// Type alias matching R's convention.
pub type SEXP = *mut SexprecCore;

// ---------------------------------------------------------------------------
// NA/NaN helpers
// ---------------------------------------------------------------------------

/// Check if a double is R's NA.
#[inline]
pub fn R_IsNA(x: c_double) -> bool {
    x.to_bits() == R_NA_BIT_PATTERN
}

/// Check if a double is NaN (any NaN, not specifically R's NA).
#[inline]
pub fn ISNAN(x: c_double) -> bool {
    x.is_nan()
}

/// Check if a double is R's NA or NaN.
#[inline]
pub fn R_IsNaN(x: c_double) -> bool {
    ISNAN(x)
}

/// Check if a double is finite (not NA, not NaN, not Inf).
#[inline]
pub fn R_FINITE(x: c_double) -> bool {
    !x.is_nan() && !x.is_infinite()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sxpinfo_new() {
        let info = SxpInfo::new(SEXPTYPE::INTSXP);
        assert_eq!(info.type_of(), SEXPTYPE::INTSXP);
        assert!(!info.scalar());
        assert!(!info.obj());
    }

    #[test]
    fn test_sxpinfo_setters() {
        let mut info = SxpInfo::new(SEXPTYPE::REALSXP);
        info.set_scalar(true);
        assert!(info.scalar());
        assert_eq!(info.type_of(), SEXPTYPE::REALSXP);

        info.set_obj(true);
        assert!(info.obj());

        info.set_named(2);
        assert_eq!(info.named(), 2);

        info.set_gp(42);
        assert_eq!(info.gp(), 42);

        info.set_mark(true);
        assert!(info.mark());
    }

    #[test]
    fn test_sexptype_vector_check() {
        assert!(SEXPTYPE::LGLSXP.is_vector_type());
        assert!(SEXPTYPE::INTSXP.is_vector_type());
        assert!(SEXPTYPE::REALSXP.is_vector_type());
        assert!(SEXPTYPE::CPLXSXP.is_vector_type());
        assert!(SEXPTYPE::STRSXP.is_vector_type());
        assert!(SEXPTYPE::VECSXP.is_vector_type());
        assert!(SEXPTYPE::RAWSXP.is_vector_type());
        assert!(!SEXPTYPE::NILSXP.is_vector_type());
        assert!(!SEXPTYPE::SYMSXP.is_vector_type());
        assert!(!SEXPTYPE::LISTSXP.is_vector_type());
    }

    #[test]
    fn test_sexptype_list_check() {
        assert!(SEXPTYPE::LISTSXP.is_list_type());
        assert!(SEXPTYPE::LANGSXP.is_list_type());
        assert!(!SEXPTYPE::VECSXP.is_list_type());
        assert!(!SEXPTYPE::NILSXP.is_list_type());
    }

    #[test]
    fn test_sexptype_atomic_check() {
        assert!(SEXPTYPE::LGLSXP.is_atomic_type());
        assert!(SEXPTYPE::INTSXP.is_atomic_type());
        assert!(SEXPTYPE::REALSXP.is_atomic_type());
        assert!(SEXPTYPE::CPLXSXP.is_atomic_type());
        assert!(SEXPTYPE::STRSXP.is_atomic_type());
        assert!(SEXPTYPE::RAWSXP.is_atomic_type());
        assert!(!SEXPTYPE::VECSXP.is_atomic_type());
        assert!(!SEXPTYPE::LISTSXP.is_atomic_type());
    }

    #[test]
    fn test_r_isna() {
        let na = c_double::from_bits(R_NA_BIT_PATTERN);
        assert!(R_IsNA(na));
        assert!(!R_IsNA(f64::NAN));
        assert!(!R_IsNA(1.0));
    }

    #[test]
    fn test_r_isnan() {
        assert!(R_IsNaN(f64::NAN));
        assert!(R_IsNaN(c_double::from_bits(R_NA_BIT_PATTERN)));
        assert!(!R_IsNaN(1.0));
    }

    #[test]
    fn test_r_finite() {
        assert!(R_FINITE(1.0));
        assert!(R_FINITE(0.0));
        assert!(R_FINITE(-1e308));
        assert!(!R_FINITE(f64::INFINITY));
        assert!(!R_FINITE(f64::NEG_INFINITY));
        assert!(!R_FINITE(f64::NAN));
    }

    #[test]
    fn test_na_integer() {
        assert_eq!(NA_INTEGER, c_int::MIN);
    }

    #[test]
    fn test_sexprec_new() {
        let node = SexprecCore::new(SEXPTYPE::INTSXP);
        assert_eq!(node.sxpinfo.type_of(), SEXPTYPE::INTSXP);
    }

    #[test]
    fn test_sexprec_new_vector() {
        let node = SexprecCore::new_vector(SEXPTYPE::REALSXP, 10);
        assert_eq!(node.sxpinfo.type_of(), SEXPTYPE::REALSXP);
        unsafe {
            assert_eq!(node.data.vecsxp.length, 10);
            assert_eq!(node.data.vecsxp.truelength, 10);
        }
    }

    #[test]
    fn test_sexprec_size() {
        // Verify the struct is a reasonable size
        assert!(std::mem::size_of::<SexprecCore>() >= 48);
    }
}
