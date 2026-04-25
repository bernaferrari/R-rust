//! Typed numeric/logical views over SEXP vectors.
//!
//! This module is the Rust-shaped boundary for internal code that needs R's
//! numeric coercion and recycling behavior without opening raw vector buffers.

use super::ffi::{NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, SEXP, SEXPTYPE};
use super::object::Sexp;

/// Safe view over the numeric/logical vectors accepted by R's arithmetic group.
///
/// It centralizes exact type checks, recycling, and NA coercion. Callers still
/// decide operation-specific result types and NA propagation semantics.
#[derive(Clone, Copy)]
pub(crate) struct NumericVector<'a> {
    sexp: Sexp<'a>,
}

impl<'a> NumericVector<'a> {
    pub(crate) fn from_raw(raw: SEXP) -> Option<Self> {
        let sexp = Sexp::from_raw(raw)?;
        Self::new(sexp)
    }

    pub(crate) fn new(sexp: Sexp<'a>) -> Option<Self> {
        match sexp.typeof_() {
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => Some(Self { sexp }),
            _ => None,
        }
    }

    pub(crate) fn len(self) -> R_xlen_t {
        self.sexp.len()
    }

    pub(crate) fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub(crate) fn typeof_(self) -> SEXPTYPE {
        self.sexp.typeof_()
    }

    pub(crate) fn needs_real_with(self, other: Self) -> bool {
        self.typeof_() == SEXPTYPE::REALSXP || other.typeof_() == SEXPTYPE::REALSXP
    }

    /// R's binary vector recycling result length.
    ///
    /// Returns zero when either side is empty; otherwise the longer length.
    pub(crate) fn recycled_len_with(self, other: Self) -> R_xlen_t {
        match (self.len(), other.len()) {
            (0, _) | (_, 0) => 0,
            (a, b) => a.max(b),
        }
    }

    pub(crate) fn recycled_index(self, i: R_xlen_t) -> Option<R_xlen_t> {
        let n = self.len();
        if n == 0 { None } else { Some(i % n) }
    }

    /// Read an element using R's integer/logical-to-real coercion.
    pub(crate) fn real_at(self, i: R_xlen_t) -> f64 {
        let Some(idx) = self.recycled_index(i) else {
            return NA_REAL;
        };
        match self.typeof_() {
            SEXPTYPE::REALSXP => self.sexp.real_elt(idx).unwrap_or(NA_REAL),
            SEXPTYPE::INTSXP => match self.sexp.integer_elt(idx) {
                Some(NA_INTEGER) | None => NA_REAL,
                Some(v) => v as f64,
            },
            SEXPTYPE::LGLSXP => match self.sexp.logical_elt(idx) {
                Some(NA_LOGICAL) | None => NA_REAL,
                Some(v) => v as f64,
            },
            _ => NA_REAL,
        }
    }

    /// Read an integer-like element from integer or logical vectors.
    pub(crate) fn int_at(self, i: R_xlen_t) -> i32 {
        let Some(idx) = self.recycled_index(i) else {
            return NA_INTEGER;
        };
        match self.typeof_() {
            SEXPTYPE::INTSXP => self.sexp.integer_elt(idx).unwrap_or(NA_INTEGER),
            SEXPTYPE::LGLSXP => self.sexp.logical_elt(idx).unwrap_or(NA_INTEGER),
            _ => NA_INTEGER,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::Rf_allocVector3;
    use crate::sexp::ffi::{FALSE, TRUE};
    use crate::sexp::session::RSession;

    #[test]
    fn numeric_vector_rejects_non_numeric_vectors() {
        let _session = RSession::new();
        unsafe {
            let strings = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            assert!(NumericVector::from_raw(strings).is_none());
        }
    }

    #[test]
    fn numeric_vector_coerces_integer_and_logical_to_real() {
        let _session = RSession::new();
        unsafe {
            let ints = Sexp::from_raw(Rf_allocVector3(SEXPTYPE::INTSXP, 2)).unwrap();
            ints.set_integer_elt(0, 10);
            ints.set_integer_elt(1, NA_INTEGER);
            let ints = NumericVector::new(ints).unwrap();
            assert_eq!(ints.real_at(0), 10.0);
            assert_eq!(ints.real_at(1).to_bits(), NA_REAL.to_bits());

            let logicals = Sexp::from_raw(Rf_allocVector3(SEXPTYPE::LGLSXP, 2)).unwrap();
            logicals.set_logical_elt(0, TRUE);
            logicals.set_logical_elt(1, FALSE);
            let logicals = NumericVector::new(logicals).unwrap();
            assert_eq!(logicals.real_at(0), 1.0);
            assert_eq!(logicals.real_at(1), 0.0);
        }
    }

    #[test]
    fn numeric_vector_reports_recycled_binary_length() {
        let _session = RSession::new();
        unsafe {
            let a = NumericVector::from_raw(Rf_allocVector3(SEXPTYPE::INTSXP, 3)).unwrap();
            let b = NumericVector::from_raw(Rf_allocVector3(SEXPTYPE::REALSXP, 1)).unwrap();
            let empty = NumericVector::from_raw(Rf_allocVector3(SEXPTYPE::INTSXP, 0)).unwrap();

            assert_eq!(a.recycled_len_with(b), 3);
            assert_eq!(a.recycled_index(4), Some(1));
            assert_eq!(a.recycled_len_with(empty), 0);
            assert!(empty.recycled_index(0).is_none());
        }
    }
}
