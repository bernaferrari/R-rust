use super::super::ffi::SEXPTYPE;

/// Owned complex value for safe Rust/UniFFI boundaries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SexpComplex {
    pub real: f64,
    pub imaginary: f64,
}

/// Named owned attribute value.
#[derive(Debug, Clone, PartialEq)]
pub struct SexpAttribute {
    pub name: String,
    pub value: SexpValue,
}

/// Owned projection of the R metadata commonly needed by embedders.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SexpMetadata {
    pub names: Option<Vec<Option<String>>>,
    pub dim: Option<Vec<i32>>,
    pub class: Option<Vec<Option<String>>>,
    pub levels: Option<Vec<Option<String>>>,
    pub attributes: Vec<SexpAttribute>,
}

/// Owned, Rust-shaped projection of an R object.
///
/// This keeps R's broad SEXP categories recognizable while removing raw
/// pointer lifetimes from embedding and mobile boundaries.
#[derive(Debug, Clone, PartialEq)]
pub enum SexpValue {
    Null,
    Logical(Option<bool>),
    Integer(Option<i32>),
    Real(Option<f64>),
    LogicalVector(Vec<Option<bool>>),
    IntegerVector(Vec<Option<i32>>),
    RealVector(Vec<Option<f64>>),
    StringVector(Vec<Option<String>>),
    RawVector(Vec<u8>),
    ComplexVector(Vec<Option<SexpComplex>>),
    List(Vec<SexpValue>),
    Attributed {
        value: Box<SexpValue>,
        metadata: SexpMetadata,
    },
    Unsupported {
        type_name: String,
    },
}

pub(super) const OWNED_VALUE_ATTRIBUTE_DEPTH_LIMIT: usize = 8;

pub(super) fn sexptype_name(t: SEXPTYPE) -> &'static str {
    match t {
        SEXPTYPE::NILSXP => "NULL",
        SEXPTYPE::SYMSXP => "symbol",
        SEXPTYPE::LISTSXP => "pairlist",
        SEXPTYPE::CLOSXP => "closure",
        SEXPTYPE::ENVSXP => "environment",
        SEXPTYPE::PROMSXP => "promise",
        SEXPTYPE::LANGSXP => "language object",
        SEXPTYPE::SPECIALSXP => "special primitive",
        SEXPTYPE::BUILTINSXP => "builtin primitive",
        SEXPTYPE::CHARSXP => "character scalar",
        SEXPTYPE::LGLSXP => "logical vector",
        SEXPTYPE::INTSXP => "integer vector",
        SEXPTYPE::REALSXP => "real vector",
        SEXPTYPE::CPLXSXP => "complex vector",
        SEXPTYPE::STRSXP => "string vector",
        SEXPTYPE::DOTSXP => "dots",
        SEXPTYPE::ANYSXP => "any",
        SEXPTYPE::VECSXP => "generic vector",
        SEXPTYPE::EXPRSXP => "expression vector",
        SEXPTYPE::BCODESXP => "bytecode",
        SEXPTYPE::EXTPTRSXP => "external pointer",
        SEXPTYPE::WEAKREFSXP => "weak reference",
        SEXPTYPE::RAWSXP => "raw vector",
        SEXPTYPE::S4SXP => "S4 object",
        _ => "SEXP",
    }
}
