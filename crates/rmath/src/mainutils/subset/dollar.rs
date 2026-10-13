use super::*;

// ---------------------------------------------------------------------------
// fixSubset3Args -- prepare arguments for `$`
// ---------------------------------------------------------------------------

/// Fix up arguments for the `$` operator. Translates the second argument
/// (a symbol or string) into a single-element character vector.
pub unsafe fn fixSubset3Args(call: SEXP, args: SEXP, env: SEXP, syminp: *mut SEXP) -> SEXP {
    unsafe {
        let input = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _input_guard = protect(input);
        let mut nlist = CADR(args);

        if isPromise(nlist) {
            nlist = crate::sexp::envir::forcePromise(nlist);
        }

        if isSymbol(nlist) {
            if !syminp.is_null() {
                *syminp = nlist;
            }
            SET_STRING_ELT(input, 0, PRINTNAME(nlist));
        } else if isString(nlist) {
            if length_int(nlist) != 1 {
                r_error("invalid subscript length");
            }
            SET_STRING_ELT(input, 0, STRING_ELT(nlist, 0));
        } else {
            let _ = call;
            r_error("invalid subscript type for $");
        }

        /* Replace the second argument with a string */
        let new_args = crate::mainutils::duplicate::shallow_duplicate(args);
        SETCAR(CDR(new_args), input);
        new_args
    }
}

// ---------------------------------------------------------------------------
// do_subset3 -- the `$` subset operator
// ---------------------------------------------------------------------------

/// The `$` subset operator. Evaluates only the first argument; the second
/// is a symbol to be matched, not evaluated.
pub unsafe fn do_subset3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        let _ = op;

        checkArity(op, args);
        let fixed_args = fixSubset3Args(call, args, env, ptr::null_mut());
        let _fixed_args_guard = protect(fixed_args);

        /* DispatchOrEval internal generic: $ */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"$\0".as_ptr() as *const c_char,
            fixed_args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        let _ans_guard = protect(ans);
        if !isNull(ans) && !isNull(CAR(ans)) && !isNull(CADR(fixed_args)) {
            ans = R_subset3_dflt(CAR(ans), STRING_ELT(CADR(fixed_args), 0), call);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// R_subset3_dflt -- default method for `$`

/// Signal the partial-match warning condition for `$` subsetting (unique
/// partial match of `input` to `target`), gated on
/// `options(warnPartialMatchDollar = TRUE)`.
#[inline(always)]
unsafe fn warn_partial_match_dollar_condition(call: SEXP, input: SEXP, target: SEXP) {
    unsafe {
        if !crate::mainutils::options::logical_option_enabled(c"warnPartialMatchDollar") {
            return;
        }
        let cond =
            crate::mainutils::errors::R_makePartialMatchWarningCondition(call, input, target);
        let _cond_guard = protect(cond);
        crate::mainutils::errors::R_signalWarningCondition(cond);
    }
}

/// Default method for `$`. Performs partial matching on pair-list and
/// vector-list names, and also handles environment subsetting.
pub unsafe fn R_subset3_dflt(x: SEXP, input: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let _input_guard = protect(input);
        let mut x = x;
        let _x_guard = protect(x);

        /* Get the length of the input string for partial matching */
        let slen = {
            let c = translateChar(input);
            if c.is_null() {
                0
            } else {
                std::ffi::CStr::from_ptr(c).to_bytes().len()
            }
        };

        // GNU subset.c: S4 classes extending environment store the env in .xData.
        if crate::mainutils::coerce::IS_S4_OBJECT(x) != 0 && TYPEOF(x) == SEXPTYPE::OBJSXP {
            x = crate::mainutils::subassign::R_getS4DataSlot(x, SEXPTYPE::ANYSXP.as_c_int());
            if x.is_null() || x == R_NilValue() {
                std::panic::panic_any(RError {
                    message: "$ operator not defined for this S4 class".to_string(),
                });
            }
            let _extracted = protect(x);
        }

        /* Pair-list / language / nil case */
        if isPairListOrNil(x) {
            let mut xmatch: SEXP = R_NilValue();
            let mut havematch: c_int = 0;
            let mut y = x;
            while !isNull(y) {
                match pstrmatch(TAG(y), input, slen) {
                    pmatch::EXACT_MATCH => {
                        let result = CAR(y);
                        RAISE_NAMED(result, NAMED(x));
                        return result;
                    }
                    pmatch::PARTIAL_MATCH => {
                        havematch += 1;
                        xmatch = y;
                    }
                    pmatch::NO_MATCH => {}
                }
                y = CDR(y);
            }
            if havematch == 1 {
                /* unique partial match */
                warn_partial_match_dollar_condition(call, input, TAG(xmatch));
                let result = CAR(xmatch);
                RAISE_NAMED(result, NAMED(x));
                return result;
            }
            return R_NilValue();
        }

        /* Vector list case */
        if isVectorList(x) {
            let nlist = getAttrib(x, sym_Names());
            let n = xlength(nlist);
            let mut imatch: R_xlen_t = -1;
            let mut havematch: c_int = 0;

            for i in 0..n {
                match pstrmatch(STRING_ELT(nlist, i), input, slen) {
                    pmatch::EXACT_MATCH => {
                        let result = VECTOR_ELT(x, i);
                        RAISE_NAMED(result, NAMED(x));
                        return result;
                    }
                    pmatch::PARTIAL_MATCH => {
                        havematch += 1;
                        if havematch == 1 {
                            /* For partial matches, mark NAMEDMAX to prevent aliasing */
                            let val = VECTOR_ELT(x, i);
                            ENSURE_NAMEDMAX(val);
                            SET_VECTOR_ELT(x, i, val);
                        }
                        imatch = i;
                    }
                    pmatch::NO_MATCH => {}
                }
            }

            if havematch == 1 {
                /* unique partial match */
                warn_partial_match_dollar_condition(call, input, STRING_ELT(nlist, imatch));
                let result = VECTOR_ELT(x, imatch);
                RAISE_NAMED(result, NAMED(x));
                return result;
            }
            return R_NilValue();
        }

        /* Environment case */
        if isEnvironment(x) {
            let sym = installTrChar(input);
            let mut y = R_findVarInFrame(x, sym);
            if isPromise(y) {
                let owner = crate::sexp::owner::OwnerToken::current()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let promise = owner
                    .sexp(y)
                    .and_then(crate::sexp::object::Sexp::into_owned)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let authority = crate::sexp::owner::StoredOwner::from_value(&promise)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                let _pin = owner
                    .pin()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                y = crate::sexp::envir::forcePromise(promise.as_raw());
                authority
                    .require_active()
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
            if y != R_UnboundValue() {
                if NAMED(y) != 0 {
                    ENSURE_NAMEDMAX(y);
                } else {
                    RAISE_NAMED(y, NAMED(x));
                }
                return y;
            }
            return R_NilValue();
        }

        /* Atomic vector case */
        if isVectorAtomic(x) {
            let _ = call;
            crate::mainutils::errors::errorcall_str(
                call,
                "$ operator is invalid for atomic vectors",
            );
        }

        /* Default: not subsettable */
        errorcallNotSubsettable(x, call);
        R_NilValue() /* unreachable */
    }
}
