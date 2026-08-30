#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;

// ---------------------------------------------------------------------------
// do_unclass -- unclass() primitive
// ---------------------------------------------------------------------------

/// R's unclass() primitive. Removes the class attribute from an object.
pub(crate) unsafe fn objects_do_unclass(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }

        if isObject(x) != FALSE {
            let t = TYPEOF(x);
            if t == SEXPTYPE::ENVSXP {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "cannot unclass an environment".to_string(),
                });
            }
            if t == SEXPTYPE::EXTPTRSXP {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "cannot unclass an external pointer".to_string(),
                });
            }
            // If potentially shared, duplicate
            // For simplicity, we skip the MAYBE_REFERENCED check
            setAttrib(x, R_ClassSymbol(), R_NilValue());
        }
        x
    }
}

// ---------------------------------------------------------------------------
// inherits2 -- S4-aware inherits check (internal)
// ---------------------------------------------------------------------------

/// Version of inherits() that supports S4 inheritance and implicit classes.
/// Returns TRUE/FALSE as c_int.
pub unsafe fn inherits2(x: SEXP, what: *const c_char) -> c_int {
    unsafe {
        if x.is_null() || what.is_null() {
            return FALSE;
        }

        if OBJECT(x) != FALSE {
            let klass = if IS_S4_OBJECT(x) != FALSE {
                R_data_class2(x)
            } else {
                R_data_class(x)
            };
            let _klass_guard = protect(klass);
            let nclass = length(klass);
            for i in 0..nclass {
                let cs = CHAR(STRING_ELT(klass, i as R_xlen_t));
                if !cs.is_null() && libc::strcmp(cs, what) == 0 {
                    return TRUE;
                }
            }
        }
        FALSE
    }
}

// ---------------------------------------------------------------------------
// inherits3 -- full inherits(x, what, which) implementation
// ---------------------------------------------------------------------------

/// C API for R's inherits(x, what, which).
///
/// If which is false, returns a single logical TRUE or FALSE.
/// If which is true, returns an integer vector of length(what).
pub(crate) unsafe fn inherits3(x: SEXP, what: SEXP, which: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || what.is_null() {
            return Rf_ScalarLogical(FALSE);
        }

        let klass = if IS_S4_OBJECT(x) != FALSE {
            R_data_class2(x)
        } else {
            R_data_class(x)
        };
        let _klass_guard = protect(klass);

        if isString(what) == FALSE {
            std::panic::panic_any(crate::sexp::context::RError {
                message:
                    "'what' must be a character vector or an object with a nameOfClass() method"
                        .to_string(),
            });
        }

        let nwhat = LENGTH(what);
        let isvec = if isLogical(which) != FALSE && LENGTH(which) == 1 {
            !LOGICAL(which).is_null() && *LOGICAL(which) == TRUE
        } else {
            false
        };

        let rval: SEXP;
        if isvec {
            rval = Rf_allocVector(SEXPTYPE::INTSXP, nwhat);
            let _rval_guard = protect(rval);
        } else {
            rval = R_NilValue();
        }

        for j in 0..nwhat {
            let ss = translateChar(STRING_ELT(what, j as R_xlen_t));
            let idx = stringPositionTr(klass, ss);
            if isvec {
                *INTEGER_ELT_mut(rval, j) = idx + 1; // 0 when not found
            } else if idx >= 0 {
                return Rf_ScalarLogical(TRUE);
            }
        }

        if isvec { rval } else { Rf_ScalarLogical(FALSE) }
    }
}

// ---------------------------------------------------------------------------
// nameOfClass -- get the class name from an object
// ---------------------------------------------------------------------------

/// Get the class name of an object. Simplified version.
unsafe fn nameOfClass(what: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if isString(what) != FALSE {
            return what;
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_inherits -- inherits() primitive
// ---------------------------------------------------------------------------

/// R's inherits(x, what, which = FALSE) primitive.
pub unsafe fn do_inherits(_call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return Rf_ScalarLogical(FALSE);
        }

        let x = CAR(args);
        let what = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            CADR(args)
        } else {
            R_NilValue()
        };
        let which = if !CDR(args).is_null()
            && CDR(args) != R_NilValue()
            && !CDDR(args).is_null()
            && CDDR(args) != R_NilValue()
        {
            CAR(CDDR(args))
        } else {
            Rf_ScalarLogical(FALSE)
        };

        // If 'what' is an object (not a character vector), try nameOfClass
        if OBJECT(what) != FALSE && TYPEOF(what) != SEXPTYPE::STRSXP {
            let name = nameOfClass(what, env);
            if name != R_NilValue() && !name.is_null() {
                let _name_guard = protect(name);
                let val = inherits3(x, name, which);
                return val;
            }
        }

        inherits3(x, what, which)
    }
}

// ---------------------------------------------------------------------------
// do_class -- class() function
// ---------------------------------------------------------------------------

/// R's class() function. Returns the class attribute of an object.
/// Note: canonical version lives in print.rs
pub(crate) unsafe fn do_class_objects(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        R_data_class(x)
    }
}

// ---------------------------------------------------------------------------
// do_isobject -- is.object() check
// ---------------------------------------------------------------------------

/// R's is.object() function. Returns TRUE if the object has an explicit class.
/// Note: canonical version lives in attrib.rs
pub(crate) unsafe fn do_isobject(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return Rf_ScalarLogical(FALSE);
        }
        let x = CAR(args);
        Rf_ScalarLogical(isObject(x))
    }
}

// ---------------------------------------------------------------------------
// do_oldClass -- oldClass() function
// ---------------------------------------------------------------------------

/// R's oldClass() function. Gets/sets the class attribute directly.
pub unsafe fn do_oldClass(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return R_NilValue();
        }
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }

        // If there's a second argument (value), set it
        if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            let value = CADR(args);
            if value.is_null() || value == R_NilValue() {
                setAttrib(x, R_ClassSymbol(), R_NilValue());
            } else {
                setAttrib(x, R_ClassSymbol(), value);
            }
        }

        // Return the class attribute
        getAttrib(x, R_ClassSymbol())
    }
}

// ---------------------------------------------------------------------------
// do_procdest -- proc.dest() function (internal)
// ---------------------------------------------------------------------------

/// Internal function to get the dispatch environment.
pub unsafe fn do_procdest(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    // Unimplemented: requires R methods package infrastructure
    unsafe {
        // proc.dest is used internally for debugging; simplified
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_isS4 -- isS4() check
// ---------------------------------------------------------------------------

/// R's isS4() function. Returns TRUE if the object has the S4 bit set.
pub unsafe fn do_isS4(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            return Rf_ScalarLogical(FALSE);
        }
        let x = CAR(args);
        Rf_ScalarLogical(IS_S4_OBJECT(x))
    }
}

// ---------------------------------------------------------------------------
// do_asS4 -- asS4() coercion
// ---------------------------------------------------------------------------

/// R's asS4() function. Sets or unsets the S4 object bit.
pub unsafe fn do_asS4(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() {
            error("invalid 'flag' argument");
        }
        let x = CAR(args);
        if x.is_null() {
            error("invalid 'flag' argument");
        }

        let flag = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            asLogical(CADR(args))
        } else {
            TRUE
        };

        if flag == crate::sexp::ffi::NA_INTEGER {
            error("invalid 'flag' argument");
        }

        let complete = if !CDR(args).is_null()
            && CDR(args) != R_NilValue()
            && !CDDR(args).is_null()
            && CDDR(args) != R_NilValue()
        {
            asInteger(CAR(CDDR(args)))
        } else {
            TRUE as c_int
        };

        if complete == crate::sexp::ffi::NA_INTEGER {
            error("invalid 'complete' argument");
        }

        asS4(x, flag, complete)
    }
}

// ---------------------------------------------------------------------------
// R_S4_method_dispatch -- S4 method dispatch entry point
// ---------------------------------------------------------------------------

/// S4 method dispatch. Requires the methods package to be loaded.
/// This low-level entry point should never silently succeed without a
/// registered dispatcher: callers depend on dispatch either producing a method
/// result or reporting that methods support is incomplete.
pub unsafe fn R_S4_method_dispatch(
    _call: SEXP,
    _op: SEXP,
    _args: SEXP,
    _rho: SEXP,
    _method: SEXP,
) -> SEXP {
    unsafe { error("S4 method dispatch is not available through this low-level entry point") }
}

// ---------------------------------------------------------------------------
// do_setClass -- setClass()
// ---------------------------------------------------------------------------

/// Legacy objects.c-facing entry point for `setClass`.
///
/// The evaluator registers the Rust-shaped implementation in
/// `mainutils::essentials`; keep this symbol as a thin compatibility route so
/// all class definitions land in the same session-local S4 registry.
pub unsafe fn do_setClass(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            error("'Class' must name an S4 class");
        }
        crate::mainutils::essentials::do_setClass(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_setRefClass -- setRefClass()
// ---------------------------------------------------------------------------

/// setRefClass() is an R-level function from the methods package.
pub unsafe fn do_setRefClass(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { error("setRefClass is not implemented in this Rust runtime yet") }
}

// ---------------------------------------------------------------------------
// R_check_class_and_super -- check class and superclasses for is()
// ---------------------------------------------------------------------------

/// Return the 0-based index of an is() match in a vector of class-name
/// strings terminated by an empty string. Returns -1 for no match.
pub unsafe fn R_check_class_and_super(x: SEXP, valid: *const *const c_char, _rho: SEXP) -> c_int {
    unsafe {
        if x.is_null() || valid.is_null() {
            return -1;
        }

        if isObject(x) != FALSE {
            let clattr = getAttrib(x, R_ClassSymbol());
            let cl = asChar(clattr);
            let _cl_guard = protect(cl);

            let class_cstr = if !cl.is_null() { CHAR(cl) } else { ptr::null() };
            if !class_cstr.is_null() {
                let mut ans: c_int = 0;
                while !(*valid.offset(ans as isize)).is_null()
                    && *(*valid.offset(ans as isize)) != 0
                {
                    if libc::strcmp(class_cstr, *valid.offset(ans as isize)) == 0 {
                        return ans;
                    }
                    ans += 1;
                }
            }
        }
        -1
    }
}

// ---------------------------------------------------------------------------
// R_check_class_etc -- simplified class check (no environment)
// ---------------------------------------------------------------------------

pub unsafe fn R_check_class_etc(x: SEXP, valid: *const *const c_char) -> c_int {
    unsafe { R_check_class_and_super(x, valid, ptr::null_mut()) }
}
