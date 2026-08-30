#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables, unused_imports)]

use super::*;

// ---------------------------------------------------------------------------
// do_usemethod -- UseMethod() primitive (SPECIALSXP)
// ---------------------------------------------------------------------------

/// R's UseMethod() primitive. This is a SPECIALSXP that implements the
/// full UseMethod dispatch protocol.
pub unsafe fn do_usemethod(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        // UseMethod takes two arguments: generic and (optionally) object
        let generic_arg = CAR(args);
        let obj_arg = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            CADR(args)
        } else {
            R_NilValue()
        };

        // Validate generic argument
        if generic_arg.is_null() || generic_arg == R_MissingArg() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "there must be a 'generic' argument".to_string(),
            });
        }

        // generic should be a character string -- in full impl we would eval it
        // Assuming it's already evaluated (promise or string)
        let generic_sexp = if TYPEOF(generic_arg) == SEXPTYPE::PROMSXP {
            // Force the promise
            generic_arg // simplified: would need eval
        } else {
            generic_arg
        };

        if isString(generic_sexp) == FALSE || LENGTH(generic_sexp) != 1 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'generic' argument must be a character string".to_string(),
            });
        }

        let generic_cstr = translateChar(STRING_ELT(generic_sexp, 0));
        if generic_cstr.is_null() || *generic_cstr == 0 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'generic' argument must be a non-empty character string".to_string(),
            });
        }

        // Get the calling context
        let cptr = R_GlobalContext();
        if cptr.is_null() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'UseMethod' used in an inappropriate fashion".to_string(),
            });
        }

        // Determine callenv and defenv
        let callenv = if !cptr.is_null() {
            // sysparent in our context struct is an int, not SEXP.
            // In the full C implementation, sysparent is an environment.
            // Using env as fallback.
            env
        } else {
            env
        };

        let defenv = topenv(R_NilValue(), env);

        // Get the object
        let obj = if !obj_arg.is_null() && obj_arg != R_NilValue() && obj_arg != R_MissingArg() {
            obj_arg
        } else {
            GetObject(cptr)
        };

        let mut ans: SEXP = ptr::null_mut();
        if usemethod(
            generic_cstr,
            obj,
            call,
            CDR(args),
            env,
            callenv,
            defenv,
            &mut ans,
        ) == 1
        {
            // Method was found and dispatched
            return ans;
        }

        // No method found -- construct error message
        let klass = R_data_class2(obj);
        let _klass_guard = protect(klass);
        let nclass = length(klass);

        if nclass == 0 {
            let msg = format!(
                "no applicable method for '{}' applied to an object of class \"\"",
                std::ffi::CStr::from_ptr(generic_cstr).to_string_lossy()
            );
            std::panic::panic_any(crate::sexp::context::RError { message: msg });
        }

        let mut class_str = String::new();
        for i in 0..nclass {
            if i > 0 {
                class_str.push_str(", ");
            }
            let cs = translateChar(STRING_ELT(klass, i as R_xlen_t));
            if !cs.is_null() {
                class_str.push_str(&std::ffi::CStr::from_ptr(cs).to_string_lossy());
            }
        }

        let msg = format!(
            "no applicable method for '{}' applied to an object of class \"{}\"",
            std::ffi::CStr::from_ptr(generic_cstr).to_string_lossy(),
            class_str
        );
        std::panic::panic_any(crate::sexp::context::RError { message: msg });
    }
}

// ---------------------------------------------------------------------------
// readS3VarsFromFrame -- read S3 dispatch variables from the frame
// ---------------------------------------------------------------------------

/// Read the S3 dispatch variables (.Generic, .Group, .Class, .Method,
/// .GenericCallEnv, .GenericDefEnv) from the method's evaluation frame.
pub unsafe fn readS3VarsFromFrame(
    frame: SEXP,
    generic: *mut SEXP,
    group: *mut SEXP,
    klass: *mut SEXP,
    method: *mut SEXP,
    callenv: *mut SEXP,
    defenv: *mut SEXP,
) {
    unsafe {
        if frame.is_null() {
            return;
        }

        let dot_generic_sym = sym(".Generic");
        let dot_group_sym = sym(".Group");
        let dot_class_sym = sym(".Class");
        let dot_method_sym = sym(".Method");
        let dot_callenv_sym = sym(".GenericCallEnv");
        let dot_defenv_sym = sym(".GenericDefEnv");

        if !generic.is_null() {
            *generic = crate::sexp::envir::R_findVarInFrame(frame, dot_generic_sym);
        }
        if !group.is_null() {
            *group = crate::sexp::envir::R_findVarInFrame(frame, dot_group_sym);
        }
        if !klass.is_null() {
            *klass = crate::sexp::envir::R_findVarInFrame(frame, dot_class_sym);
        }
        if !method.is_null() {
            *method = crate::sexp::envir::R_findVarInFrame(frame, dot_method_sym);
        }
        if !callenv.is_null() {
            *callenv = crate::sexp::envir::R_findVarInFrame(frame, dot_callenv_sym);
        }
        if !defenv.is_null() {
            *defenv = crate::sexp::envir::R_findVarInFrame(frame, dot_defenv_sym);
        }
    }
}

// ---------------------------------------------------------------------------
// do_nextmethod -- NextMethod() .Internal
// ---------------------------------------------------------------------------

/// R's NextMethod() function, called via .Internal.
///
/// Implements the NextMethod protocol for S3 dispatch.
#[allow(clippy::if_same_then_else)]
pub unsafe fn do_nextmethod(call: SEXP, _op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let cptr = R_GlobalContext();
        if cptr.is_null() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'NextMethod' called from outside a function".to_string(),
            });
        }

        // Mark this context as generic while preserving its function/return
        // bits; NextMethod still needs to rediscover the current closure frame.
        (*cptr).callflag |= crate::sexp::context::ctxt_flags::CTXT_GENERIC;

        // In this port S3 dispatch variables are installed directly in the
        // method closure frame. Prefer that frame, falling back to sysparent
        // for older call paths that still mirror C's context layout.
        let sysp = if !(*cptr).cloenv.is_null() {
            (*cptr).cloenv
        } else {
            (*cptr).sysparent
        };

        // Walk the context stack to find the function context matching sysp
        let mut found_cptr: *mut RCNTXT = ptr::null_mut();
        let mut ctx_iter = cptr;
        while !ctx_iter.is_null() {
            let cf = (*ctx_iter).callflag;
            if (cf & crate::sexp::context::ctxt_flags::CTXT_FUNCTION) != 0 {
                // Check if this context matches
                if (*ctx_iter).cloenv == sysp || (*ctx_iter).cloenv.is_null() {
                    found_cptr = ctx_iter;
                    break;
                }
            }
            ctx_iter = (*ctx_iter).nextcontext;
        }

        if found_cptr.is_null() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'NextMethod' called from outside a function".to_string(),
            });
        }

        // Duplicate the call (parity with C: use shallow_duplicate)
        let mut newcall = crate::mainutils::duplicate::shallow_duplicate((*found_cptr).call);
        if newcall.is_null() || newcall == R_NilValue() {
            return R_NilValue();
        }
        let _newcall_guard = protect(newcall);

        // Check that the call's first element is a symbol
        if TYPEOF(CAR(newcall)) != SEXPTYPE::SYMSXP {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "'NextMethod' called from an anonymous function".to_string(),
            });
        }

        // Read S3 vars from frame
        let mut generic: SEXP = R_UnboundValue();
        let mut group: SEXP = R_UnboundValue();
        let mut klass: SEXP = R_UnboundValue();
        let mut method: SEXP = R_UnboundValue();
        let mut callenv: SEXP = R_UnboundValue();
        let mut defenv: SEXP = R_UnboundValue();

        readS3VarsFromFrame(
            sysp,
            &mut generic,
            &mut group,
            &mut klass,
            &mut method,
            &mut callenv,
            &mut defenv,
        );

        // Resolve promise environments (C: eval promise if PROMSXP)
        if TYPEOF(callenv) == SEXPTYPE::PROMSXP {
            callenv = Rf_eval(callenv, R_BaseEnv());
        } else if callenv == R_UnboundValue() {
            callenv = env;
        }
        if TYPEOF(defenv) == SEXPTYPE::PROMSXP {
            defenv = Rf_eval(defenv, R_BaseEnv());
        } else if defenv == R_UnboundValue() {
            defenv = R_GlobalEnv();
        }

        let s_callfun = (*found_cptr).callfun;
        if TYPEOF(s_callfun) != SEXPTYPE::CLOSXP {
            if s_callfun == R_UnboundValue() {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "no calling generic was found: was a method called directly?"
                        .to_string(),
                });
            } else {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!(
                        "'function' is not a function, but of type {}",
                        TYPEOF(s_callfun)
                    ),
                });
            }
        }

        let formals = FORMALS(s_callfun);
        // Use patchArgsByActuals instead of raw promiseargs
        let supplied_args =
            if (*found_cptr).promiseargs.is_null() || (*found_cptr).promiseargs == R_NilValue() {
                CDR((*found_cptr).call)
            } else {
                (*found_cptr).promiseargs
            };
        let mut matchedarg = patchArgsByActuals(formals, supplied_args, (*found_cptr).cloenv);
        let mut _matchedarg_guard = protect(matchedarg);

        // Handle ... arguments (C: s = CADDR(args), check R_DotsSymbol)
        if !args.is_null() && args != R_NilValue() {
            let dotarg = CADDR(args);
            if dotarg == R_DotsSymbol_fn() {
                let t = crate::sexp::envir::R_findVarInFrame(env, dotarg);
                if !t.is_null() && t != R_NilValue() && t != R_MissingArg() {
                    (*t).sxpinfo.set_type(SEXPTYPE::LISTSXP);
                    let s = matchmethargs(matchedarg, t);
                    drop(_matchedarg_guard);
                    matchedarg = s;
                    _matchedarg_guard = protect(matchedarg);
                    newcall = fixcall(newcall, matchedarg);
                }
            } else {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "wrong argument ...".to_string(),
                });
            }
        }

        // Get klass if unbound
        if klass == R_UnboundValue() {
            let obj = GetObject(found_cptr);
            if isObject(obj) == FALSE {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "object not specified".to_string(),
                });
            }
            klass = getAttrib(obj, R_ClassSymbol());
        }

        // Validate generic
        if generic == R_UnboundValue() {
            generic = Rf_eval(CAR(args), env);
        }
        if generic == R_NilValue() || generic.is_null() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "generic function not specified".to_string(),
            });
        }
        let _generic_guard = protect(generic);

        if isString(generic) == FALSE || LENGTH(generic) != 1 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "invalid generic argument to 'NextMethod'".to_string(),
            });
        }

        let generic_cstr = CHAR(STRING_ELT(generic, 0));
        if generic_cstr.is_null() || *generic_cstr == 0 {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "generic function not specified".to_string(),
            });
        }

        // Determine group dispatch
        let mut basename = generic;
        let mut group_val = group;
        if group_val == R_UnboundValue() {
            group_val = R_BlankScalarString_placeholder();
            // basename stays as generic
        } else {
            if isString(group_val) == FALSE || LENGTH(group_val) != 1 {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "invalid 'group' argument found in 'NextMethod'".to_string(),
                });
            }
            let gc = CHAR(STRING_ELT(group_val, 0));
            if !gc.is_null() && *gc != 0 {
                basename = group_val;
            }
        }
        let _group_guard = protect(group_val);

        if (args.is_null() || args == R_NilValue())
            && let Some(value) = simple_next_method_dispatch(
                (*found_cptr).call,
                generic,
                klass,
                method,
                env,
                callenv,
                defenv,
            )
        {
            return value;
        }

        // Find current method in .Class
        let mut nextfun: SEXP = R_NilValue();
        let mut nextfunSignature: SEXP = R_NilValue();

        let mut b: *const c_char = ptr::null();
        let mut method_idx: c_int = 0;
        if method != R_UnboundValue() {
            if isString(method) == FALSE {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "wrong value for .Method".to_string(),
                });
            }
            for ii in 0..LENGTH(method) {
                let bb = translateChar(STRING_ELT(method, ii as R_xlen_t));
                if !bb.is_null() && *bb != 0 && libc::strlen(bb) > 0 {
                    b = bb;
                    method_idx = ii;
                    break;
                }
            }
            for jj in method_idx..LENGTH(method) {
                let bb = translateChar(STRING_ELT(method, jj as R_xlen_t));
                if !bb.is_null() && libc::strlen(bb) > 0 && libc::strcmp(b, bb) != 0 {
                    crate::mainutils::errors::Rf_warning(
                        b"Incompatible methods ignored\0".as_ptr() as *const c_char,
                    );
                }
            }
        } else {
            b = CHAR(PRINTNAME(CAR((*found_cptr).call)));
        }

        // Find matching signature in .Class
        let _vmax = crate::sexp::memory_ext::vmaxget();
        let sb = translateChar(STRING_ELT(basename, 0));
        let mut found_sig: c_int = FALSE;
        let nclass = length(klass);
        let mut j: c_int = 0;

        if !sb.is_null() && !b.is_null() {
            for jj in 0..nclass {
                let sk = translateChar(STRING_ELT(klass, jj as R_xlen_t));
                if equalS3Signature(b, sb, sk) != FALSE {
                    found_sig = TRUE;
                    j = jj;
                    break;
                }
            }
        }

        if found_sig != FALSE {
            j += 1;
        } else {
            j = 0;
        }

        // Search for the next method
        let sg = translateChar(STRING_ELT(generic, 0));
        let mut i: c_int = 0;

        for ii in j..nclass {
            let sk = translateChar(STRING_ELT(klass, ii as R_xlen_t));
            nextfunSignature = crate::mainutils::names::installS3Signature(sg, sk);
            nextfun = R_LookupMethod(nextfunSignature, env, callenv, defenv);
            if isFunction(nextfun) != FALSE {
                i = ii;
                break;
            }
            // If not found and we have a group, try group method
            if group_val != R_UnboundValue() {
                let sb2 = translateChar(STRING_ELT(basename, 0));
                nextfunSignature = crate::mainutils::names::installS3Signature(sb2, sk);
                nextfun = R_LookupMethod(nextfunSignature, env, callenv, defenv);
                if isFunction(nextfun) != FALSE {
                    i = ii;
                    break;
                }
            }
        }

        if isFunction(nextfun) == FALSE {
            // Try default method
            nextfunSignature = crate::mainutils::names::installS3Signature(
                sg,
                b"default\0".as_ptr() as *const c_char,
            );
            nextfun = R_LookupMethod(nextfunSignature, env, callenv, defenv);

            // If there is no default method, try the generic itself,
            // provided it is primitive or a wrapper for a .Internal function
            if isFunction(nextfun) == FALSE {
                let t = Rf_install(sg);
                nextfun = crate::sexp::envir::R_findVar(t, env);
                if TYPEOF(nextfun) == SEXPTYPE::PROMSXP {
                    let _nextfun_eval_guard = protect(nextfun);
                    nextfun = Rf_eval(nextfun, env);
                }
                if isFunction(nextfun) == FALSE {
                    crate::sexp::memory_ext::vmaxset(_vmax);
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: "no method to invoke".to_string(),
                    });
                }
                if TYPEOF(nextfun) == SEXPTYPE::CLOSXP {
                    let internal_val = crate::sexp::accessors::INTERNAL(t);
                    if internal_val != R_NilValue() {
                        nextfun = internal_val;
                    } else {
                        nextfun = getPrimitive(t);
                        if nextfun == R_NilValue() {
                            crate::sexp::memory_ext::vmaxset(_vmax);
                            std::panic::panic_any(crate::sexp::context::RError {
                                message: "no method to invoke".to_string(),
                            });
                        }
                    }
                }
            }
        }

        let _nextfun_guard = protect(nextfun);
        let s = stringSuffix(klass, i);
        let _s_guard = protect(s);
        setAttrib(s, sym("previous"), klass);

        // Set up method name (C: duplicate(method) and update elements)
        let method_name: SEXP;
        let _method_name_guard;
        if method != R_UnboundValue() {
            method_name = crate::mainutils::duplicate::duplicate(method);
            _method_name_guard = protect(method_name);
            for jj in 0..LENGTH(method_name) {
                let mc = CHAR(STRING_ELT(method_name, jj as R_xlen_t));
                if !mc.is_null() && *mc != 0 && libc::strlen(mc) > 0 {
                    SET_STRING_ELT(method_name, jj as R_xlen_t, PRINTNAME(nextfunSignature));
                }
            }
        } else {
            method_name = PRINTNAME(nextfunSignature);
            _method_name_guard = protect(method_name);
        }

        // Create S3 vars
        let newvars = createS3Vars(generic, group_val, s, method_name, callenv, defenv);
        let _newvars_guard = protect(newvars);

        SETCAR(newcall, nextfunSignature);

        // Fixup sysparent (C: PR#15267 fix)
        let global_ctx = R_GlobalContext();
        if !global_ctx.is_null() {
            (*global_ctx).sysparent = callenv;
        }

        let ans = applyMethod(newcall, nextfun, matchedarg, env, newvars);

        crate::sexp::memory_ext::vmaxset(_vmax);
        ans
    }
}

