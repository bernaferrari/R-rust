//! c() and unlist(): do_c, do_c_dflt, do_unlist, do_unlist_default — extracted verbatim from the former single-file module.
#![allow(unused_imports)]
use super::*;
use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::eval::attrib_core::{R_data_class, getAttrib, isObject, setAttrib};
use crate::eval::dispatch::DispatchOrEval;
use crate::eval::dispatch::promiseArgs;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_LOGICAL, NA_REAL, R_xlen_t, Rbyte, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// do_c -- the c() primitive (SPECIALSXP)
// ---------------------------------------------------------------------------

/// R's `c()` builtin.  Attempts method dispatch; falls back to `do_c_dflt`.
pub unsafe fn do_c(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let args = R_listCompact(args, true);

        // S3 method dispatch: check if any arg is an object with a "c" method
        let mut method: SEXP = R_NilValue();
        let mut a = args;
        while !a.is_null() && a != R_NilValue() && method == R_NilValue() {
            let obj = crate::eval::eval::Rf_eval(CAR(a), env);
            if isObject(obj) != 0 {
                let classlist = R_data_class(obj);
                let classlen = Rf_length(classlist);
                for i in 0..classlen {
                    let class_str = translateChar(STRING_ELT(classlist, i as R_xlen_t));
                    let s = std::ffi::CStr::from_ptr(class_str).to_str().unwrap_or("");
                    let method_name = format!("c.{}\0", s);
                    let sym =
                        crate::sexp::symbol::Rf_install(method_name.as_ptr() as *const c_char);
                    let classmethod = crate::mainutils::objects::R_LookupMethod(
                        sym,
                        env,
                        env,
                        crate::sexp::globals::R_BaseEnv(),
                    );
                    if classmethod != crate::sexp::globals::R_UnboundValue() {
                        method = classmethod;
                        break;
                    }
                }
            }
            a = CDR(a);
        }

        if method != R_NilValue() {
            return crate::eval::closure::applyClosure(call, method, args, env, R_NilValue(), 0);
        }

        do_c_dflt(call, op, args, env)
    }
}

// ---------------------------------------------------------------------------
// do_c_dflt -- default method for c()
// ---------------------------------------------------------------------------

/// Default implementation of `c()` when no S3/S4 method is found.
pub unsafe fn do_c_dflt(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut usenames: bool = true;
        let mut recurse: bool = false;

        // Handle empty args — c() with no args returns NULL
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        // Extract optional arguments (recursive, use.names)
        let args = c_Extract_opt(args, &mut recurse, &mut usenames, call);
        let _args_guard = protect(args);

        // Determine the type of the returned value.
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };

        let mut t = args;
        while !t.is_null() && t != R_NilValue() {
            let value = resolve_promise(CAR(t));
            if usenames && data.ans_nnames == 0 {
                if !Rf_isNull(TAG(t)) != 0 {
                    data.ans_nnames = 1;
                } else {
                    data.ans_nnames = HasNames(value) as R_xlen_t;
                }
            }
            AnswerType(value, recurse, usenames, &mut data, call);
            t = CDR(t);
        }

        // Determine the result mode from the accumulated flags
        let mode = ans_flags_to_mode(data.ans_flags);

        // If no actual values were found, return NULL
        if data.ans_length == 0 {
            return R_NilValue();
        }

        let ans = checked_allocVector(mode, data.ans_length);
        let _ans_guard = protect(ans);
        data.ans_ptr = ans;
        data.ans_length = 0;

        // Reset t to iterate args again
        t = args;

        // Fill in the values
        if mode == SEXPTYPE::VECSXP || mode == SEXPTYPE::EXPRSXP {
            if !recurse {
                let mut a = args;
                while !a.is_null() && a != R_NilValue() {
                    ListAnswer(resolve_promise(CAR(a)), 0, &mut data, call);
                    a = CDR(a);
                }
            } else {
                let mut a = args;
                while !a.is_null() && a != R_NilValue() {
                    ListAnswer(resolve_promise(CAR(a)), 1, &mut data, call);
                    a = CDR(a);
                }
            }
            data.ans_length = xlength(ans);
        } else if mode == SEXPTYPE::STRSXP {
            StringAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::CPLXSXP {
            ComplexAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::REALSXP {
            RealAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::RAWSXP {
            RawAnswer(args, &mut data, call);
        } else if mode == SEXPTYPE::LGLSXP {
            LogicalAnswer(args, &mut data, call);
        } else {
            // integer
            IntegerAnswer(args, &mut data, call);
        }

        // Reset t again for name extraction
        t = args;

        // Build and attach the names attribute
        if data.ans_nnames != 0 && data.ans_length > 0 {
            data.ans_names = checked_allocVector(SEXPTYPE::STRSXP, data.ans_length as R_xlen_t);
            let _ans_names_guard = protect(data.ans_names);
            data.ans_nnames = 0;
            let mut a = args;
            while !a.is_null() && a != R_NilValue() {
                let mut nameData = NameData { count: 0, seqno: 0 };
                NewExtractNames(
                    resolve_promise(CAR(a)),
                    R_NilValue(),
                    TAG(a),
                    recurse as c_int,
                    &mut data,
                    &mut nameData,
                );
                a = CDR(a);
            }
            let names_sym = crate::eval::attrib_core::R_NamesSymbol();
            setAttrib(ans, names_sym, data.ans_names);
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_unlist -- the unlist() builtin
// ---------------------------------------------------------------------------

/// R's `unlist()` builtin.  Attempts method dispatch; falls back to default.
pub unsafe fn do_unlist(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        // Attempt method dispatch.
        // DispatchOrEval internal generic: unlist
        let mut ans: SEXP = ptr::null_mut();
        let generic = std::ffi::CString::new("unlist").unwrap_or_default();
        // DispatchOrEval returns 1 if dispatched (result in ans), 0 if not.
        let dispatched = DispatchOrEval(call, op, generic.as_ptr(), args, env, &mut ans, 0, 0);
        if dispatched != 0 {
            return ans;
        }

        // Method dispatch has failed; run the default code with evaluated args.
        do_unlist_default(call, op, ans, env)
    }
}

/// Default implementation of unlist().
pub unsafe fn do_unlist_default(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // unlist takes: (x, recursive, use.names)
        // args is a pairlist: (x, recursive, use.names)
        let x_arg = CAR(args);
        let recurse_arg = CADR(args);
        let usenames_arg = CADDR(args);

        let mut recurse: bool = true;
        let mut usenames: bool = true;
        let lenient: bool = true;

        // Extract recurse from the second argument
        if !recurse_arg.is_null() && recurse_arg != R_NilValue() && TYPEOF(recurse_arg) == LGLSXP_I
        {
            let v = *LOGICAL(recurse_arg);
            if v != NA_LOGICAL {
                recurse = v != 0;
            }
        }

        // Extract usenames from the third argument
        if !usenames_arg.is_null()
            && usenames_arg != R_NilValue()
            && TYPEOF(usenames_arg) == LGLSXP_I
        {
            let v = *LOGICAL(usenames_arg);
            if v != NA_LOGICAL {
                usenames = v != 0;
            }
        }

        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };

        let mut n: R_xlen_t = 0;
        let names_sym = crate::eval::attrib_core::R_NamesSymbol();

        if isNewList(x_arg) {
            n = xlength(x_arg);
            if usenames && !Rf_isNull(getAttrib(x_arg, names_sym)) != 0 {
                data.ans_nnames = 1;
            }
            for i in 0..n {
                if usenames && data.ans_nnames == 0 {
                    data.ans_nnames = HasNames(VECTOR_ELT(x_arg, i)) as R_xlen_t;
                }
                AnswerType(VECTOR_ELT(x_arg, i), recurse, usenames, &mut data, call);
            }
        } else if isList(x_arg) != 0 {
            let mut t = x_arg;
            while !t.is_null() && t != R_NilValue() {
                if usenames && data.ans_nnames == 0 {
                    if !Rf_isNull(TAG(t)) != 0 {
                        data.ans_nnames = 1;
                    } else {
                        data.ans_nnames = HasNames(CAR(t)) as R_xlen_t;
                    }
                }
                AnswerType(CAR(t), recurse, usenames, &mut data, call);
                t = CDR(t);
            }
        } else {
            if lenient || isVector(x_arg) != 0 {
                return x_arg;
            }
            let msg = std::ffi::CString::new("argument not a list").unwrap_or_default();
            std::panic::panic_any(crate::sexp::context::RError {
                message: msg.into_string().unwrap_or_default(),
            });
        }

        // Determine the result mode
        let mode = ans_flags_to_mode(data.ans_flags);

        let ans = checked_allocVector(mode, data.ans_length);
        let _ans_guard = protect(ans);
        data.ans_ptr = ans;
        data.ans_length = 0;

        // Fill in the values
        if mode == SEXPTYPE::VECSXP || mode == SEXPTYPE::EXPRSXP {
            if !recurse {
                if TYPEOF(x_arg) == VECSXP_I {
                    for i in 0..n {
                        ListAnswer(VECTOR_ELT(x_arg, i), 0, &mut data, call);
                    }
                } else if TYPEOF(x_arg) == LISTSXP_I {
                    let mut a = x_arg;
                    while !a.is_null() && a != R_NilValue() {
                        ListAnswer(CAR(a), 0, &mut data, call);
                        a = CDR(a);
                    }
                }
            } else {
                ListAnswer(x_arg, 1, &mut data, call);
            }
            data.ans_length = xlength(ans);
        } else if mode == SEXPTYPE::STRSXP {
            StringAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::CPLXSXP {
            ComplexAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::REALSXP {
            RealAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::RAWSXP {
            RawAnswer(x_arg, &mut data, call);
        } else if mode == SEXPTYPE::LGLSXP {
            LogicalAnswer(x_arg, &mut data, call);
        } else {
            IntegerAnswer(x_arg, &mut data, call);
        }

        // Build and attach names
        if data.ans_nnames != 0 && data.ans_length > 0 {
            data.ans_names = checked_allocVector(SEXPTYPE::STRSXP, data.ans_length as R_xlen_t);
            let _ans_names_guard = protect(data.ans_names);

            if !recurse {
                if TYPEOF(x_arg) == VECSXP_I {
                    let names = getAttrib(x_arg, names_sym);
                    data.ans_nnames = 0;
                    let mut nameData = NameData { count: 0, seqno: 0 };
                    for i in 0..n {
                        NewExtractNames(
                            VECTOR_ELT(x_arg, i),
                            R_NilValue(),
                            ItemName(names, i),
                            0,
                            &mut data,
                            &mut nameData,
                        );
                    }
                } else if TYPEOF(x_arg) == LISTSXP_I {
                    data.ans_nnames = 0;
                    let mut nameData = NameData { count: 0, seqno: 0 };
                    let mut a = x_arg;
                    while !a.is_null() && a != R_NilValue() {
                        NewExtractNames(CAR(a), R_NilValue(), TAG(a), 0, &mut data, &mut nameData);
                        a = CDR(a);
                    }
                }
            } else {
                data.ans_nnames = 0;
                let mut nameData = NameData { count: 0, seqno: 0 };
                NewExtractNames(
                    x_arg,
                    R_NilValue(),
                    R_NilValue(),
                    1,
                    &mut data,
                    &mut nameData,
                );
            }

            setAttrib(ans, names_sym, data.ans_names);
        }

        ans
    }
}
