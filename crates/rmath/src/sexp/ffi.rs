#![forbid(unsafe_code)]
#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Runtime scalar types and canonical initialized Rust object records.
//!
//! Language type tags retain their numeric encoding. Object headers use a
//! checked Rust enum body and do not require a native SEXPREC layout.

use std::os::raw::{c_double, c_int, c_void};

use super::heap::NodeLink;

// ---------------------------------------------------------------------------
// Primitive type aliases (centralized from duplicates)
// ---------------------------------------------------------------------------

/// R's NA_INTEGER sentinel value.
pub const NA_INTEGER: c_int = c_int::MIN;

/// R's NA_LOGICAL sentinel value.
pub const NA_LOGICAL: c_int = c_int::MIN;

/// GNU `R_ValueOfNA()`: high word `0x7ff00000`, low word `1954`
/// (`arithmetic.c`). Quiet-NaN variants with the same payload are also NA.
pub const R_NA_BIT_PATTERN: u64 = 0x7FF00000000007A2;

/// Low 32 bits of GNU `NA_REAL` (`arithmetic.c` `lw = 1954`).
pub const R_NA_PAYLOAD: u32 = 1954;

/// R's NA_REAL sentinel — derived from R_NA_BIT_PATTERN so they cannot drift.
pub const NA_REAL: c_double = f64::from_bits(R_NA_BIT_PATTERN);

/// GNU `R_IsNA`: any NaN whose payload is 1954, including both signaling
/// (`0x7ff00000000007a2`) and quiet (`0x7ff80000000007a2`) encodings.
#[inline]
pub fn is_na_real(x: f64) -> bool {
    x.is_nan() && (x.to_bits() as u32) == R_NA_PAYLOAD
}

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
/// Values retain R's language and serialization type-tag encoding.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SEXPTYPE(pub c_int);

impl PartialEq<c_int> for SEXPTYPE {
    #[inline]
    fn eq(&self, other: &c_int) -> bool {
        self.0 == *other
    }
}

impl PartialEq<SEXPTYPE> for c_int {
    #[inline]
    fn eq(&self, other: &SEXPTYPE) -> bool {
        *self == other.0
    }
}

impl From<SEXPTYPE> for c_int {
    #[inline]
    fn from(t: SEXPTYPE) -> c_int {
        t.0
    }
}

impl From<c_int> for SEXPTYPE {
    #[inline]
    fn from(v: c_int) -> SEXPTYPE {
        SEXPTYPE(v)
    }
}

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
    pub const S4SXP: SEXPTYPE = SEXPTYPE(25);
    pub const FUNSXP: SEXPTYPE = SEXPTYPE(99);

    /// Return the raw C integer tag value.
    #[inline]
    pub const fn as_c_int(self) -> c_int {
        self.0
    }

    /// Check if this type is a vector type (has length/trueLength fields).
    #[inline]
    pub fn is_vector_type(self) -> bool {
        matches!(self.0, 10 | 13 | 14 | 15 | 16 | 19 | 20 | 21 | 24)
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

    /// GNU `sxpinfo.trace` (bit 26). Memory profiling (`tracemem`) uses this
    /// bit. Function tracing (`.primTrace`) uses a separate gp bit.
    #[inline]
    pub fn set_trace(&mut self, v: bool) {
        self.type_and_flags = (self.type_and_flags & !(1 << 26)) | ((v as u32) << 26);
    }

    #[inline]
    pub fn set_named(&mut self, n: u8) {
        self.type_and_flags = (self.type_and_flags & !(0x03 << 29)) | ((n as u32 & 0x03) << 29);
    }

    #[inline]
    pub fn set_gcgen(&mut self, v: u8) {
        let next = v & 0x01;
        let prev = self.gcgen();
        self.type_and_flags = (self.type_and_flags & !(1 << 28)) | ((next as u32) << 28);
        // The torture sweep visits old nodes via a side bitmap. Generation
        // changes on stack temporaries are not slab nodes; the note is a no-op
        // when this address is not inside an arena page.
        if prev != next {
            let node = std::ptr::from_mut(self)
                .cast::<u8>()
                .wrapping_byte_sub(std::mem::offset_of!(SexprecCore, sxpinfo))
                .cast::<SexprecCore>();
            crate::sexp::memory::note_slab_generation(node, next);
        }
    }
}

// ---------------------------------------------------------------------------
// Type-specific initialized data records
// ---------------------------------------------------------------------------

/// Primitive function data (offset into function table).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Primsxp {
    pub offset: c_int,
}

/// Symbol data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symsxp {
    pub pname: NodeLink,
    pub value: NodeLink,
    pub internal: NodeLink,
}

/// List/cons cell data (LISTSXP and LANGSXP).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listsxp {
    pub carval: NodeLink,
    pub cdrval: NodeLink,
    pub tagval: NodeLink,
}

/// Environment data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envsxp {
    pub frame: NodeLink,
    pub enclos: NodeLink,
    pub hashtab: NodeLink,
}

/// Closure data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Closxp {
    pub formals: NodeLink,
    pub body: NodeLink,
    pub env: NodeLink,
}

/// Promise data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Promsxp {
    pub value: NodeLink,
    pub expr: NodeLink,
    pub env: NodeLink,
}

/// Vector data header (length and true length).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vecsxp {
    pub length: R_xlen_t,
    pub truelength: R_xlen_t,
}

/// An external address is opaque; only its protected value and tag are graph edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtPtrBody {
    pub address: *mut c_void,
    pub protected: NodeLink,
    pub tag: NodeLink,
}

/// Semantic fields accepted by copied-header graph operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeField {
    Attribute,
    ListCar,
    ListCdr,
    ListTag,
    SymbolName,
    SymbolValue,
    SymbolInternal,
    ClosureFormals,
    ClosureBody,
    ClosureEnvironment,
    EnvironmentFrame,
    EnvironmentEnclosure,
    EnvironmentHashTable,
    PromiseValue,
    PromiseExpression,
    PromiseEnvironment,
    ExternalProtected,
    ExternalTag,
}

// ---------------------------------------------------------------------------
// Canonical typed node bodies
// ---------------------------------------------------------------------------

/// Canonical initialized type-specific storage. The Rust enum discriminant
/// selects the data arm; a mismatched accessor fails safely instead of
/// reinterpreting header bytes. Graph fields retain nonowning, exact allocation identities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeBody {
    Vector(Vecsxp),
    List(Listsxp),
    Symbol(Symsxp),
    Closure(Closxp),
    Environment(Envsxp),
    Promise(Promsxp),
    Primitive(Primsxp),
    ExtPtr(ExtPtrBody),
    #[default]
    Other,
}

impl NodeBody {
    /// Initialize the arm used by this object's semantic type family.
    pub fn for_kind(kind: SEXPTYPE) -> Self {
        let null = NodeLink::null();
        match kind {
            SEXPTYPE::CHARSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP
            | SEXPTYPE::BCODESXP
            | SEXPTYPE::RAWSXP => Self::Vector(Vecsxp::default()),
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP | SEXPTYPE::WEAKREFSXP => {
                Self::List(Listsxp {
                    carval: null,
                    cdrval: null,
                    tagval: null,
                })
            }
            SEXPTYPE::SYMSXP => Self::Symbol(Symsxp {
                pname: null,
                value: null,
                internal: null,
            }),
            SEXPTYPE::CLOSXP => Self::Closure(Closxp {
                formals: null,
                body: null,
                env: null,
            }),
            SEXPTYPE::ENVSXP => Self::Environment(Envsxp {
                frame: null,
                enclos: null,
                hashtab: null,
            }),
            SEXPTYPE::PROMSXP => Self::Promise(Promsxp {
                value: null,
                expr: null,
                env: null,
            }),
            SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP => Self::Primitive(Primsxp { offset: 0 }),
            SEXPTYPE::EXTPTRSXP => Self::ExtPtr(ExtPtrBody {
                address: std::ptr::null_mut(),
                protected: null,
                tag: null,
            }),
            _ => Self::Other,
        }
    }

    #[inline]
    pub fn vector(&self) -> Vecsxp {
        match self {
            Self::Vector(value) => *value,
            _ => panic!("expected vector body"),
        }
    }

    #[inline]
    pub fn vector_mut(&mut self) -> &mut Vecsxp {
        match self {
            Self::Vector(value) => value,
            _ => panic!("expected vector body"),
        }
    }

    #[inline]
    pub fn list(&self) -> Listsxp {
        match self {
            Self::List(value) => *value,
            _ => panic!("expected list body"),
        }
    }

    #[inline]
    pub fn list_mut(&mut self) -> &mut Listsxp {
        match self {
            Self::List(value) => value,
            _ => panic!("expected list body"),
        }
    }

    #[inline]
    pub fn symbol(&self) -> Symsxp {
        match self {
            Self::Symbol(value) => *value,
            _ => panic!("expected symbol body"),
        }
    }

    #[inline]
    pub fn symbol_mut(&mut self) -> &mut Symsxp {
        match self {
            Self::Symbol(value) => value,
            _ => panic!("expected symbol body"),
        }
    }

    #[inline]
    pub fn closure(&self) -> Closxp {
        match self {
            Self::Closure(value) => *value,
            _ => panic!("expected closure body"),
        }
    }

    #[inline]
    pub fn closure_mut(&mut self) -> &mut Closxp {
        match self {
            Self::Closure(value) => value,
            _ => panic!("expected closure body"),
        }
    }

    #[inline]
    pub fn environment(&self) -> Envsxp {
        match self {
            Self::Environment(value) => *value,
            _ => panic!("expected environment body"),
        }
    }

    #[inline]
    pub fn environment_mut(&mut self) -> &mut Envsxp {
        match self {
            Self::Environment(value) => value,
            _ => panic!("expected environment body"),
        }
    }

    #[inline]
    pub fn promise(&self) -> Promsxp {
        match self {
            Self::Promise(value) => *value,
            _ => panic!("expected promise body"),
        }
    }

    #[inline]
    pub fn promise_mut(&mut self) -> &mut Promsxp {
        match self {
            Self::Promise(value) => value,
            _ => panic!("expected promise body"),
        }
    }

    #[inline]
    pub fn primitive(&self) -> Primsxp {
        match self {
            Self::Primitive(value) => *value,
            _ => panic!("expected primitive body"),
        }
    }

    #[inline]
    pub fn primitive_mut(&mut self) -> &mut Primsxp {
        match self {
            Self::Primitive(value) => value,
            _ => panic!("expected primitive body"),
        }
    }

    #[inline]
    pub fn extptr(&self) -> ExtPtrBody {
        match self {
            Self::ExtPtr(value) => *value,
            _ => panic!("expected extptr body"),
        }
    }

    #[inline]
    pub fn extptr_mut(&mut self) -> &mut ExtPtrBody {
        match self {
            Self::ExtPtr(value) => value,
            _ => panic!("expected extptr body"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The core SEXPREC structure -- the fundamental R object.
///
/// This is the unified scalar/vector node. Its canonical Rust body owns
/// an initialized type-specific record, independent of any native layout.
/// Vector records hold their shape; the heap owns their typed element bytes.
#[derive(Clone, Copy)]
pub struct SexprecCore {
    pub sxpinfo: SxpInfo,
    pub attrib: NodeLink,
    pub gengc_next_node: *mut SexprecCore,
    pub gengc_prev_node: *mut SexprecCore,
    pub data: NodeBody,
}

impl SexprecCore {
    /// Create a new SexprecCore with the given type.
    pub fn new(sexptype: SEXPTYPE) -> Self {
        SexprecCore {
            sxpinfo: SxpInfo::new(sexptype),
            attrib: NodeLink::null(),
            gengc_next_node: std::ptr::null_mut(),
            gengc_prev_node: std::ptr::null_mut(),
            data: NodeBody::for_kind(sexptype),
        }
    }

    /// Copy a semantic edge without interpreting a mismatched body family.
    pub(crate) fn edge(&self, field: EdgeField) -> Option<NodeLink> {
        match (field, self.data) {
            (EdgeField::Attribute, _) => Some(self.attrib),
            (EdgeField::ListCar, NodeBody::List(body)) => Some(body.carval),
            (EdgeField::ListCdr, NodeBody::List(body)) => Some(body.cdrval),
            (EdgeField::ListTag, NodeBody::List(body)) => Some(body.tagval),
            (EdgeField::SymbolName, NodeBody::Symbol(body)) => Some(body.pname),
            (EdgeField::SymbolValue, NodeBody::Symbol(body)) => Some(body.value),
            (EdgeField::SymbolInternal, NodeBody::Symbol(body)) => Some(body.internal),
            (EdgeField::ClosureFormals, NodeBody::Closure(body)) => Some(body.formals),
            (EdgeField::ClosureBody, NodeBody::Closure(body)) => Some(body.body),
            (EdgeField::ClosureEnvironment, NodeBody::Closure(body)) => Some(body.env),
            (EdgeField::EnvironmentFrame, NodeBody::Environment(body)) => Some(body.frame),
            (EdgeField::EnvironmentEnclosure, NodeBody::Environment(body)) => Some(body.enclos),
            (EdgeField::EnvironmentHashTable, NodeBody::Environment(body)) => Some(body.hashtab),
            (EdgeField::PromiseValue, NodeBody::Promise(body)) => Some(body.value),
            (EdgeField::PromiseExpression, NodeBody::Promise(body)) => Some(body.expr),
            (EdgeField::PromiseEnvironment, NodeBody::Promise(body)) => Some(body.env),
            (EdgeField::ExternalProtected, NodeBody::ExtPtr(body)) => Some(body.protected),
            (EdgeField::ExternalTag, NodeBody::ExtPtr(body)) => Some(body.tag),
            _ => None,
        }
    }

    /// Replace a semantic edge in a detached header snapshot.
    pub(crate) fn set_edge(&mut self, field: EdgeField, value: NodeLink) -> Option<()> {
        match (field, &mut self.data) {
            (EdgeField::Attribute, _) => self.attrib = value,
            (EdgeField::ListCar, NodeBody::List(body)) => body.carval = value,
            (EdgeField::ListCdr, NodeBody::List(body)) => body.cdrval = value,
            (EdgeField::ListTag, NodeBody::List(body)) => body.tagval = value,
            (EdgeField::SymbolName, NodeBody::Symbol(body)) => body.pname = value,
            (EdgeField::SymbolValue, NodeBody::Symbol(body)) => body.value = value,
            (EdgeField::SymbolInternal, NodeBody::Symbol(body)) => body.internal = value,
            (EdgeField::ClosureFormals, NodeBody::Closure(body)) => body.formals = value,
            (EdgeField::ClosureBody, NodeBody::Closure(body)) => body.body = value,
            (EdgeField::ClosureEnvironment, NodeBody::Closure(body)) => body.env = value,
            (EdgeField::EnvironmentFrame, NodeBody::Environment(body)) => body.frame = value,
            (EdgeField::EnvironmentEnclosure, NodeBody::Environment(body)) => body.enclos = value,
            (EdgeField::EnvironmentHashTable, NodeBody::Environment(body)) => body.hashtab = value,
            (EdgeField::PromiseValue, NodeBody::Promise(body)) => body.value = value,
            (EdgeField::PromiseExpression, NodeBody::Promise(body)) => body.expr = value,
            (EdgeField::PromiseEnvironment, NodeBody::Promise(body)) => body.env = value,
            (EdgeField::ExternalProtected, NodeBody::ExtPtr(body)) => body.protected = value,
            (EdgeField::ExternalTag, NodeBody::ExtPtr(body)) => body.tag = value,
            _ => return None,
        }
        Some(())
    }

    /// Create a new vector SexprecCore with length.
    pub fn new_vector(sexptype: SEXPTYPE, length: R_xlen_t) -> Self {
        let mut node = Self::new(sexptype);
        *node.data.vector_mut() = Vecsxp {
            length,
            truelength: length,
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
    is_na_real(x)
}

/// Check if a double is NaN (any NaN, not specifically R's NA).
#[inline]
pub fn ISNAN(x: c_double) -> bool {
    x.is_nan()
}

/// Check if a double is NaN but not NA (R semantics: R_IsNaN excludes NA).
#[inline]
pub fn R_IsNaN(x: c_double) -> bool {
    x.is_nan() && !is_na_real(x)
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
        assert!(SEXPTYPE::BCODESXP.is_vector_type());
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
        assert!(!R_IsNaN(c_double::from_bits(R_NA_BIT_PATTERN)));
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
        assert_eq!(node.data.vector().length, 10);
        assert_eq!(node.data.vector().truelength, 10);
    }

    #[test]
    fn canonical_body_construction_selects_initialized_type_families() {
        for kind in [
            SEXPTYPE::CHARSXP,
            SEXPTYPE::LGLSXP,
            SEXPTYPE::INTSXP,
            SEXPTYPE::REALSXP,
            SEXPTYPE::CPLXSXP,
            SEXPTYPE::STRSXP,
            SEXPTYPE::VECSXP,
            SEXPTYPE::EXPRSXP,
            SEXPTYPE::BCODESXP,
            SEXPTYPE::RAWSXP,
        ] {
            let header = SexprecCore::new(kind);
            assert_eq!(header.sxpinfo.type_of(), kind);
            assert_eq!(header.data.vector().length, 0);
            assert_eq!(header.data.vector().truelength, 0);
        }
        for kind in [
            SEXPTYPE::LISTSXP,
            SEXPTYPE::LANGSXP,
            SEXPTYPE::DOTSXP,
            SEXPTYPE::WEAKREFSXP,
        ] {
            let value = SexprecCore::new(kind).data.list();
            assert!(value.carval.is_null());
            assert!(value.cdrval.is_null());
            assert!(value.tagval.is_null());
        }
        let symbol = SexprecCore::new(SEXPTYPE::SYMSXP).data.symbol();
        assert!(symbol.pname.is_null() && symbol.value.is_null() && symbol.internal.is_null());
        let closure = SexprecCore::new(SEXPTYPE::CLOSXP).data.closure();
        assert!(closure.formals.is_null() && closure.body.is_null() && closure.env.is_null());
        let environment = SexprecCore::new(SEXPTYPE::ENVSXP).data.environment();
        assert!(
            environment.frame.is_null()
                && environment.enclos.is_null()
                && environment.hashtab.is_null()
        );
        let promise = SexprecCore::new(SEXPTYPE::PROMSXP).data.promise();
        assert!(promise.value.is_null() && promise.expr.is_null() && promise.env.is_null());
        for kind in [SEXPTYPE::BUILTINSXP, SEXPTYPE::SPECIALSXP] {
            assert_eq!(SexprecCore::new(kind).data.primitive().offset, 0);
        }
        let external = SexprecCore::new(SEXPTYPE::EXTPTRSXP).data.extptr();
        assert!(external.address.is_null());
        assert!(external.protected.is_null() && external.tag.is_null());
        for kind in [
            SEXPTYPE::NILSXP,
            SEXPTYPE::ANYSXP,
            SEXPTYPE::OBJSXP,
            SEXPTYPE::FUNSXP,
            SEXPTYPE(31),
        ] {
            assert!(matches!(SexprecCore::new(kind).data, NodeBody::Other));
        }
    }

    #[test]
    fn wrong_body_projections_fail_safely_without_mutating_the_arm() {
        let copied: [fn(&NodeBody); 8] = [
            |body| {
                let _ = body.vector();
            },
            |body| {
                let _ = body.list();
            },
            |body| {
                let _ = body.symbol();
            },
            |body| {
                let _ = body.closure();
            },
            |body| {
                let _ = body.environment();
            },
            |body| {
                let _ = body.promise();
            },
            |body| {
                let _ = body.primitive();
            },
            |body| {
                let _ = body.extptr();
            },
        ];
        for projection in copied {
            assert!(std::panic::catch_unwind(|| projection(&NodeBody::Other)).is_err());
        }
        let mutable: [fn(&mut NodeBody); 8] = [
            |body| {
                let _ = body.vector_mut();
            },
            |body| {
                let _ = body.list_mut();
            },
            |body| {
                let _ = body.symbol_mut();
            },
            |body| {
                let _ = body.closure_mut();
            },
            |body| {
                let _ = body.environment_mut();
            },
            |body| {
                let _ = body.promise_mut();
            },
            |body| {
                let _ = body.primitive_mut();
            },
            |body| {
                let _ = body.extptr_mut();
            },
        ];
        for projection in mutable {
            let mut body = NodeBody::Other;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| projection(&mut body)));
            assert!(result.is_err());
            assert!(matches!(body, NodeBody::Other));
        }
        let mut vector = NodeBody::for_kind(SEXPTYPE::REALSXP);
        let copied = vector.vector();
        vector.vector_mut().length = 7;
        assert_eq!(copied.length, 0);
        assert_eq!(vector.vector().length, 7);
        assert!(std::panic::catch_unwind(|| vector.list()).is_err());
    }

    #[test]
    #[should_panic(expected = "expected vector body")]
    fn vector_header_constructor_rejects_nonvector_type_families() {
        let _ = SexprecCore::new_vector(SEXPTYPE::SYMSXP, 1);
    }

    #[test]
    fn test_sexprec_size() {
        // Verify the struct is a reasonable size
        assert!(std::mem::size_of::<SexprecCore>() >= 48);
    }
}
