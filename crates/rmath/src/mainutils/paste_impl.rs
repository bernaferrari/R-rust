#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Runtime handlers for paste(), paste0(), file.path(), and format.info().

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::mainutils::relop::PRIMVAL;
use crate::sexp::accessors::{CADDR, CADR, CAR, STRING_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_allocVector, Rf_isNull};
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R_MIN_DIGITS_OPT: minimum value for the digits argument.
const R_MIN_DIGITS_OPT: c_int = 1;

/// R_MAX_DIGITS_OPT: maximum value for the digits argument.
const R_MAX_DIGITS_OPT: c_int = 22;

/// SEXPTYPE integer values for match patterns.
const LGLSXP: c_int = 10;
const INTSXP: c_int = 13;
const REALSXP: c_int = 14;
const CPLXSXP: c_int = 15;
const STRSXP: c_int = 16;
const RAWSXP: c_int = 24;

// ---------------------------------------------------------------------------
// Local helpers for native R operations
// ---------------------------------------------------------------------------

fn checkArity(op: SEXP, args: SEXP) {
    crate::mainutils::relop::checkArity(op, args)
}


fn asInteger(x: SEXP) -> c_int {
    crate::mainutils::coerce::asInteger(x)
}


fn asBool2(x: SEXP, call: SEXP) -> bool {
    crate::mainutils::coerce::asRbool(x, call) != 0
}

fn coerceVector(s: SEXP, t: c_int) -> SEXP {
    crate::mainutils::coerce::coerceVector(s, t)
}


fn PrintDefaults() {
    crate::mainutils::print::PrintDefaults();
}






fn reconcile_fixed_real_width(
    x: &[c_double],
    width: &mut c_int,
    digits_right: c_int,
    exponent: c_int,
) {
    if x.is_empty() || exponent != 0 || digits_right < 0 {
        return;
    }
    let digits = digits_right as usize;
    for &value in x {
        let rendered = if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
            "NA".to_string()
        } else if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value.is_sign_negative() {
                "-Inf".to_string()
            } else {
                "Inf".to_string()
            }
        } else {
            format!("{value:.digits$}")
        };
        *width = (*width).max(rendered.chars().count() as c_int);
    }
}

fn eval(x: SEXP, env: SEXP) -> SEXP {
    crate::eval::eval::Rf_eval(x, env)
}

fn R_strlen(s: SEXP, _type: c_int) -> c_int {
    if s.is_null() {
        return 0;
    }
    crate::mainutils::printutils::charsxp_bytes(s).len() as c_int
}

fn R_AsCharacterSymbol() -> SEXP {
    crate::sexp::symbol::install_str("as.character")
}

// ---------------------------------------------------------------------------
// NA_STRING check
// ---------------------------------------------------------------------------

fn isNA_STRING(s: SEXP) -> bool {
    if s.is_null() {
        return true;
    }
    // GNU checks gp bit 0 of the CHARSXP header.
    crate::sexp::accessors::LEVELS(s) & 1 != 0
}

// ---------------------------------------------------------------------------
// do_paste
// ---------------------------------------------------------------------------

/// .Internal(paste (args, sep, collapse, recycle0))
/// .Internal(paste0(args,      collapse, recycle0))
#[path = "paste_impl/owned.rs"]
mod owned_paste;
#[path = "paste_impl/path.rs"]
mod owned_path;

struct PasteNative<'execution>(&'execution crate::sexp::owner::RuntimeAccess);
impl owned_paste::Native for PasteNative<'_> {
    fn initialize(
        &self,
        op: &owned_paste::Value,
        args: &owned_paste::Value,
    ) -> crate::sexp::object::SexpResult<bool> {
        self.0.with_native(|original| {
            // Native projections are kept alive by the actual owning operands.
            checkArity(op.as_raw(), args.as_raw());
            PrintDefaults();
            original.require_active()?;
            Ok(PRIMVAL(op.as_raw()) == 0)
        })
    }
    fn boolean(
        &self,
        value: &owned_paste::Value,
        call: &owned_paste::Value,
    ) -> crate::sexp::object::SexpResult<bool> {
        self.0.with_native(|original| {
            let result = asBool2(value.as_raw(), call.as_raw());
            original.require_active()?;
            Ok(result)
        })
    }
    fn coerce(
        &self,
        value: &owned_paste::Value,
        env: &owned_paste::Value,
    ) -> crate::sexp::object::SexpResult<owned_paste::Value> {
        use crate::sexp::{ffi::SEXPTYPE, object::SexpMut};
        let domain = self.0.domain();
        let allocator = self.0.allocator(&domain)?;
        if value.header().sxpinfo.obj() {
            let function = self.0.with_native(|original| {
                let symbol = R_AsCharacterSymbol();
                original.require_active()?;
                original.sexp(symbol)?.into_owned()
            })?;
            let arguments = allocator.pairlist_cell(value, &domain.nil(), &domain.nil())?;
            let call = allocator.call(&function, &arguments)?;
            self.0.with_native(|original| {
                let result = eval(call.as_raw(), env.as_raw());
                original.require_active()?;
                original.sexp(result)?.into_owned()
            })
        } else if value.typeof_() == SEXPTYPE::SYMSXP {
            let name = value.try_printname()?.into_owned()?;
            let mut result = SexpMut::try_from_checked(
                allocator.allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))?,
            )?;
            result.try_set_string_elt(0, name)?;
            Ok(result.freeze())
        } else {
            // This remains the translated coercion boundary. Its selected input
            // and original runtime remain owned through warnings and providers.
            self.0.with_native(|original| {
                let result = coerceVector(value.as_raw(), SEXPTYPE::STRSXP.as_c_int());
                original.require_active()?;
                original.sexp(result)?.into_owned()
            })
        }
    }
    fn bytes(
        &self,
        value: &owned_paste::Value,
        mode: owned_paste::Mode,
        parent: &owned_paste::Value,
    ) -> crate::sexp::object::SexpResult<owned_paste::Bytes> {
        self.0.require_active()?;
        self.0.domain().link(value)?;
        self.0.domain().link(parent)?;
        let translated = crate::sexp::encoding::translate(value, mode, parent)?;
        self.0.require_active()?;
        Ok(owned_paste::Bytes {
            data: translated.data,
            _reservation: translated.reservation,
        })
    }
    fn flags(
        &self,
        value: &owned_paste::Value,
    ) -> crate::sexp::object::SexpResult<crate::sexp::encoding::CharacterFlags> {
        self.0.require_active()?;
        self.0.domain().link(value)?;
        crate::sexp::encoding::flags(value)
    }
    fn name(&self, op: &owned_paste::Value) -> String {
        match crate::eval::primitive::raw_primitive_entry(op.as_raw()) {
            Some(_) => crate::eval::primitive::PRIMNAME(op.as_raw()).to_string(),
            None => String::new(),
        }
    }
}

pub fn do_paste(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    if op.is_null() || args.is_null() {
        return ptr::null_mut();
    }
    use crate::sexp::owner::OwnerToken;
    OwnerToken::with_current(|owner| {
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let own = |raw| {
            owner
                .sexp(raw)
                .and_then(crate::sexp::object::Sexp::into_owned)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        };
        let (call, op, args, env) = (own(call), own(op), own(args), own(env));
        let weak = owner
            .weak_owner()
            .unwrap_or_else(|| crate::sexp::context::r_error("paste requires a managed runtime"));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::sexp::owner::with_runtime(&weak, |access| {
                owned_paste::evaluate(access, &PasteNative(access), call, op, args, env)
            })
        }));
        // Revocation or foreign activation must not authenticate success/unwind.
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        match outcome {
            Ok(Ok(Ok(value))) => value.as_raw(),
            Ok(Ok(Err(crate::sexp::object::SexpError::EvaluationFailed { message }))) => {
                crate::sexp::context::r_error(message)
            }
            Ok(Ok(Err(error))) | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

// ---------------------------------------------------------------------------
// do_filepath
// ---------------------------------------------------------------------------

/// .Internal(filepath(...))
pub fn do_filepath(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    filepath_entry(args, env, false)
}

pub(crate) fn do_filepath_public(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    filepath_entry(args, env, true)
}

fn filepath_entry(args: SEXP, env: SEXP, public: bool) -> SEXP {
    use crate::sexp::owner::OwnerToken;
    OwnerToken::with_current(|owner| {
        let _pin = owner
            .pin()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let own = |raw| {
            owner
                .sexp(raw)
                .and_then(crate::sexp::object::Sexp::into_owned)
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        };
        let (args, env) = (own(args), own(env));
        let weak = owner.weak_owner().unwrap_or_else(|| {
            crate::sexp::context::r_error("file.path requires a managed runtime")
        });
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::sexp::owner::with_runtime(&weak, |access| {
                if public {
                    owned_path::public(access, &PasteNative(access), args, env)
                } else {
                    owned_path::internal(access, &PasteNative(access), args, env)
                }
            })
        }));
        owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        match outcome {
            Ok(Ok(Ok(value))) => value.as_raw(),
            Ok(Ok(Err(crate::sexp::diagnostic::EvaluationError::Diagnostic(diagnostic)))) => {
                crate::mainutils::errors::raise_owned_diagnostic(&owner, diagnostic)
            }
            Ok(Ok(Err(crate::sexp::diagnostic::EvaluationError::Access(
                crate::sexp::object::SexpError::EvaluationFailed { message },
            )))) => crate::sexp::context::r_error(message),
            Ok(Ok(Err(crate::sexp::diagnostic::EvaluationError::Access(error))))
            | Ok(Err(error)) => crate::sexp::context::r_error(error.to_string()),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
    .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

// ---------------------------------------------------------------------------
// do_formatinfo
// ---------------------------------------------------------------------------

/// format.info(obj) --> 3 integers (w,d,e) with the formatting information
///   w = total width (#{chars}) per item
///   d = #{digits} to RIGHT of "."
///   e = {0:2}.   0: Fixpoint; 1,2: exponential with 2/3 digit expon.
///
/// for complex: 2 x 3 integers for (Re, Im)
pub fn do_formatinfo(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    // Null arglists can appear in direct FFI tests; treat as no-op.
    if args.is_null() || args == R_NilValue() {
        return ptr::null_mut();
    }

    checkArity(op, args);

    let x = CAR(args);
    if x.is_null() || x == R_NilValue() {
        return ptr::null_mut();
    }
    let n = XLENGTH(x);
    PrintDefaults();

    let mut restore_digits = None;
    if Rf_isNull(CADR(args)) == 0 {
        let digits = asInteger(CADR(args));
        if digits == NA_INTEGER || digits < R_MIN_DIGITS_OPT || digits > R_MAX_DIGITS_OPT {
            return ptr::null_mut();
        }
        let old = crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
            digits,
            scipen: crate::mainutils::options::GetOptionScipen(),
            na_width: 2,
            na_width_noquote: 2,
        });
        restore_digits = Some(FormatInfoRestorePrint { old });
    }

    let nsmall = asInteger(CADDR(args));
    if nsmall == NA_INTEGER || nsmall < 0 || nsmall > 20 {
        return ptr::null_mut();
    }

    let mut w: c_int = 0;
    let mut d: c_int = 0;
    let mut e: c_int = 0;
    let mut wi: c_int = 0;
    let mut di: c_int = 0;
    let mut ei: c_int = 0;
    let mut no: c_int = 1;

    let values = || {
        crate::mainutils::essentials::owner_sexp(crate::mainutils::essentials::active_owner(), x)
    };
    let checked = |error: crate::sexp::object::SexpError| -> ! {
        crate::sexp::context::r_error(error.to_string())
    };
    match TYPEOF(x) {
        RAWSXP => {
            // GNU formatRaw: always two hex digits.
            w = 2;
        }

        LGLSXP => {
            let src = values();
            let data: Vec<c_int> = (0..n)
                .map(|i| src.try_logical_elt(i).unwrap_or_else(|e| checked(e)))
                .collect();
            w = if n > 0 {
                crate::mainutils::format::logical_field_width(data)
            } else {
                1
            };
        }

        INTSXP => {
            let src = values();
            w = if n > 0 {
                crate::mainutils::format::integer_field_width(n, |i| {
                    src.try_integer_elt(i).unwrap_or_else(|e| checked(e))
                })
            } else {
                1
            };
        }

        REALSXP => {
            no = 3;
            let src = values();
            let data: Vec<c_double> = (0..n)
                .map(|i| src.try_real_elt(i).unwrap_or_else(|e| checked(e)))
                .collect();
            if !data.is_empty() {
                let f = crate::mainutils::format::real_field(n, nsmall, |i| data[i as usize]);
                (w, d, e) = (f.w, f.d, f.e);
            }
            reconcile_fixed_real_width(&data, &mut w, d, e);
        }

        CPLXSXP => {
            no = 6;
            let src = values();
            let data: Vec<crate::sexp::ffi::Rcomplex> = (0..n)
                .map(|i| src.try_complex_elt(i).unwrap_or_else(|e| checked(e)))
                .collect();
            let f = crate::mainutils::format::complex_field(&data, nsmall);
            (w, d, e, wi, di, ei) = (f.wr, f.dr, f.er, f.wi, f.di, f.ei);
        }

        STRSXP => {
            for i in 0..n {
                if !isNA_STRING(STRING_ELT(x, i)) {
                    let il = R_strlen(STRING_ELT(x, i), 0);
                    if il > w {
                        w = il;
                    }
                }
            }
        }

        _ => {
            return ptr::null_mut();
        }
    }

    let result = Rf_allocVector(SEXPTYPE::INTSXP, no);
    let _result_guard = crate::sexp::protect::protect(result);
    let fields = [w, d, e, wi, di, ei];
    for (i, &value) in fields.iter().take(no as usize).enumerate() {
        crate::mainutils::essentials::set_integer_checked(result, i as R_xlen_t, value);
    }

    let _ = restore_digits;
    result
}

struct FormatInfoRestorePrint {
    old: crate::mainutils::format::RPrint,
}

impl Drop for FormatInfoRestorePrint {
    fn drop(&mut self) {
        crate::mainutils::format::format_set_R_print(self.old);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_paste_null_args() {
        let nil = ptr::null_mut();
        assert!(do_paste(nil, nil, nil, nil).is_null());
    }

    #[test]
    fn test_do_formatinfo_null_args() {
        let nil = ptr::null_mut();
        assert!(do_formatinfo(nil, nil, nil, nil).is_null());
    }
}

#[cfg(test)]
#[path = "paste_impl/tests.rs"]
mod owned_tests;

#[cfg(test)]
#[path = "paste_impl/filepath_tests.rs"]
mod filepath_tests;
