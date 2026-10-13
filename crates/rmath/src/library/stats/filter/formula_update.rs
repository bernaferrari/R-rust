//! update.formula machinery: updateform, dot expansion, formula/DF2formula

use super::*;

/// GNU `update.formula(y ~ x, ~ . + z)` — dot-template substitution.
pub unsafe fn do_update_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = CAR(args);
        let new = CAR(CDR(args));
        updateform(old, new)
    }
}

fn setcadr(x: SEXP, v: SEXP) {
    unsafe {
        SETCAR(CDR(x), v);
    }
}

fn setcaddr(x: SEXP, v: SEXP) {
    unsafe {
        SETCAR(CDR(CDR(x)), v);
    }
}

/// GNU `ExpandDots` (stats/src/model.c): replace `.` templates in `object`
/// with `value`, parenthesizing where operator precedence requires it.
unsafe fn expand_dots(object: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let dot = crate::sexp::symbol::Rf_install(c".".as_ptr());
        if TYPEOF(object) == SEXPTYPE::SYMSXP {
            if object == dot {
                return crate::mainutils::duplicate::Rf_duplicate(value);
            }
            return object;
        }
        if TYPEOF(object) != SEXPTYPE::LANGSXP {
            return object;
        }
        let sym = |s: &std::ffi::CStr| crate::sexp::symbol::Rf_install(s.as_ptr());
        let plus = sym(c"+");
        let minus = sym(c"-");
        let times = sym(c"*");
        let slash = sym(c"/");
        let colon = sym(c":");
        let power = sym(c"^");
        let paren = sym(c"(");
        let op = if TYPEOF(value) == SEXPTYPE::LANGSXP {
            CAR(value)
        } else {
            R_NilValue()
        };
        let len = crate::sexp::constructors::Rf_length(object);
        let head = CAR(object);
        // Wrap a `.` operand's replacement in parentheses when the value's
        // top operator binds looser than the operand position.
        let wrap = |is_dot: bool, loose: &[SEXP]| -> Option<SEXP> {
            if is_dot && loose.iter().any(|&s| s == op) {
                Some(crate::sexp::constructors::Rf_lang2(
                    paren,
                    crate::mainutils::duplicate::Rf_duplicate(value),
                ))
            } else {
                None
            }
        };
        let add_loose = [plus, minus];
        let mul_loose = [plus, minus, times, slash];
        let pow_loose = [plus, minus, times, slash, colon];
        let invalid = || crate::mainutils::essentials::base_error("invalid formula in 'update'");
        if head == plus {
            if len == 2 || len == 3 {
                setcadr(object, expand_dots(CADR(object), value));
                if len == 3 {
                    setcaddr(object, expand_dots(CADDR(object), value));
                }
            } else {
                invalid();
            }
        } else if head == minus {
            if len == 2 || len == 3 {
                if let Some(wrapped) = wrap(CADR(object) == dot, &add_loose) {
                    setcadr(object, wrapped);
                } else {
                    setcadr(object, expand_dots(CADR(object), value));
                }
                if len == 3 {
                    if let Some(wrapped) = wrap(CADDR(object) == dot, &add_loose) {
                        setcaddr(object, wrapped);
                    } else {
                        setcaddr(object, expand_dots(CADDR(object), value));
                    }
                }
            } else {
                invalid();
            }
        } else if head == times || head == slash {
            if len != 3 {
                invalid();
            }
            if let Some(wrapped) = wrap(CADR(object) == dot, &add_loose) {
                setcadr(object, wrapped);
            } else {
                setcadr(object, expand_dots(CADR(object), value));
            }
            if let Some(wrapped) = wrap(CADDR(object) == dot, &add_loose) {
                setcaddr(object, wrapped);
            } else {
                setcaddr(object, expand_dots(CADDR(object), value));
            }
        } else if head == colon {
            if len != 3 {
                invalid();
            }
            if let Some(wrapped) = wrap(CADR(object) == dot, &mul_loose) {
                setcadr(object, wrapped);
            } else {
                setcadr(object, expand_dots(CADR(object), value));
            }
            if let Some(wrapped) = wrap(CADDR(object) == dot, &add_loose) {
                setcaddr(object, wrapped);
            } else {
                setcaddr(object, expand_dots(CADDR(object), value));
            }
        } else if head == power {
            if len != 3 {
                invalid();
            }
            if let Some(wrapped) = wrap(CADR(object) == dot, &pow_loose) {
                setcadr(object, wrapped);
            } else {
                setcadr(object, expand_dots(CADR(object), value));
            }
            if let Some(wrapped) = wrap(CADDR(object) == dot, &add_loose) {
                setcaddr(object, wrapped);
            } else {
                setcaddr(object, expand_dots(CADDR(object), value));
            }
        } else {
            let mut cell = object;
            while cell != R_NilValue() && !cell.is_null() {
                SETCAR(cell, expand_dots(CAR(cell), value));
                cell = CDR(cell);
            }
        }
        object
    }
}

/// GNU `updateform` (stats/src/model.c): substitute `.` templates of `new`
/// from the corresponding side of `old`.
pub unsafe fn updateform(old: SEXP, new: SEXP) -> SEXP {
    unsafe {
        let tilde = crate::sexp::symbol::Rf_install(c"~".as_ptr());
        if TYPEOF(old) != SEXPTYPE::LANGSXP || TYPEOF(new) != SEXPTYPE::LANGSXP || CAR(new) != tilde
        {
            crate::mainutils::essentials::base_error("formula expected");
        }
        let expanded = crate::mainutils::duplicate::Rf_duplicate(new);
        let _guard = protect(expanded);
        if crate::sexp::constructors::Rf_length(old) == 3 {
            let lhs = CADR(old);
            let rhs = CADDR(old);
            if crate::sexp::constructors::Rf_length(expanded) == 2 {
                SETCDR(
                    expanded,
                    crate::sexp::constructors::Rf_cons(lhs, CDR(expanded)),
                );
            }
            setcadr(expanded, expand_dots(CADR(expanded), lhs));
            setcaddr(expanded, expand_dots(CADDR(expanded), rhs));
        } else {
            let rhs = CADR(old);
            if crate::sexp::constructors::Rf_length(expanded) == 3 {
                setcaddr(expanded, expand_dots(CADDR(expanded), rhs));
            } else {
                setcadr(expanded, expand_dots(CADR(expanded), rhs));
            }
        }
        crate::sexp::accessors::SET_ATTRIB(expanded, R_NilValue());
        let dot_env = crate::sexp::symbol::Rf_install(c".Environment".as_ptr());
        crate::sexp::attrib_core::setAttrib(
            expanded,
            dot_env,
            crate::sexp::attrib_core::getAttrib(old, dot_env),
        );
        mark_formula(expanded)
    }
}

fn parse_formula_text(s: SEXP) -> SEXP {
    unsafe {
        let mut status: std::os::raw::c_int = 0;
        let parsed = crate::mainutils::gram_main::R_ParseVector(s, -1, &mut status, R_NilValue());
        if status != 1 || parsed.is_null() || parsed == R_NilValue() || XLENGTH(parsed) < 1 {
            return R_NilValue();
        }
        mark_formula(VECTOR_ELT(parsed, 0))
    }
}

/// GNU `formula(object)` — character, language, `$formula`, or `$call`.
pub unsafe fn do_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if inherits_formula(x) {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) > 0 {
            return parse_formula_text(x);
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP {
            return mark_formula(x);
        }
        let f = named_list_elt(x, "formula");
        if !f.is_null() && f != R_NilValue() {
            return f;
        }
        let call = named_list_elt(x, "call");
        if !call.is_null() && call != R_NilValue() && TYPEOF(call) == SEXPTYPE::LANGSXP {
            return CADR(call);
        }
        R_NilValue()
    }
}

/// GNU `DF2formula(x)` — first name ~ second name.
pub unsafe fn do_df2formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) < 2 {
            return R_NilValue();
        }
        let lhs = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 0)))
            .to_string_lossy()
            .into_owned();
        let rhs = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, 1)))
            .to_string_lossy()
            .into_owned();
        let text = format!("{lhs} ~ {rhs}");
        let s = Rf_mkString(std::ffi::CString::new(text).unwrap().as_ptr());
        let _g = protect(s);
        parse_formula_text(s)
    }
}

/// GNU `replications(~ a, data)` — balanced one-factor count.
pub unsafe fn do_replications(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        let col = if TYPEOF(data) == SEXPTYPE::VECSXP && XLENGTH(data) > 0 {
            VECTOR_ELT(data, 0)
        } else {
            data
        };
        let n = XLENGTH(col) as usize;
        if n == 0 {
            return Rf_ScalarInteger(0);
        }
        let first = if TYPEOF(col) == SEXPTYPE::INTSXP {
            *INTEGER(col) as f64
        } else {
            *REAL(col)
        };
        let mut count = 0i32;
        for i in 0..n {
            let v = if TYPEOF(col) == SEXPTYPE::INTSXP {
                *INTEGER(col).add(i) as f64
            } else {
                *REAL(col).add(i)
            };
            if (v - first).abs() < 1e-12 {
                count += 1;
            }
        }
        Rf_ScalarInteger(count)
    }
}
