//! Type predicates (is.language etc), noquote, deparse, charmatch, pmatch, strtoi, strtrim

use super::*;

/// R's `is.language(x)` — TRUE for LANGSXP, SYMSXP, or EXPRSXP.
pub unsafe fn do_is_language(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(
            if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::EXPRSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

/// R's `is.call(x)` — TRUE for LANGSXP.
pub unsafe fn do_is_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::LANGSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.symbol(x)` — TRUE for SYMSXP.
pub unsafe fn do_is_symbol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::SYMSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.name(x)` — alias for is.symbol.
pub unsafe fn do_is_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_is_symbol(_call, _op, args, _rho) }
}

/// R's `is.pairlist(x)` — TRUE for LISTSXP.
pub unsafe fn do_is_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::LISTSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.function(x)` — TRUE for CLOSXP, BUILTINSXP, or SPECIALSXP.
pub unsafe fn do_is_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(
            if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

/// R's `is.expression(x)` — TRUE for EXPRSXP.
pub unsafe fn do_is_expression(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::EXPRSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.environment(x)` — TRUE for ENVSXP and S4 environment subclasses.
pub unsafe fn do_is_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            return Rf_ScalarLogical(TRUE);
        }
        if crate::mainutils::coerce::IS_S4_OBJECT(x) != 0 && TYPEOF(x) == SEXPTYPE::OBJSXP {
            let data = crate::mainutils::subassign::R_getS4DataSlot(x, SEXPTYPE::ENVSXP.as_c_int());
            return Rf_ScalarLogical(if TYPEOF(data) == SEXPTYPE::ENVSXP {
                TRUE
            } else {
                FALSE
            });
        }
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// String formatting
// ---------------------------------------------------------------------------

/// R's `noquote(x)` — mark a copy so print skips quoting.
pub unsafe fn do_noquote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        x = crate::mainutils::duplicate::shallow_duplicate_if_shared(x);
        let _x = protect(x);
        if crate::mainutils::objects::inherits2(x, c"noquote".as_ptr()) == FALSE {
            let old =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
            let old_n = if !old.is_null() && old != R_NilValue() && TYPEOF(old) == SEXPTYPE::STRSXP
            {
                XLENGTH(old)
            } else {
                0
            };
            let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, old_n + 1);
            if !class_vec.is_null() {
                let _c = protect(class_vec);
                for i in 0..old_n {
                    SET_STRING_ELT(class_vec, i, STRING_ELT(old, i));
                }
                SET_STRING_ELT(
                    class_vec,
                    old_n,
                    crate::sexp::constructors::Rf_mkChar(c"noquote".as_ptr()),
                );
                crate::sexp::attrib_core::setAttrib(
                    x,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    class_vec,
                );
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        x
    }
}

/// R's `deparse(x)` — convert an object or expression to source-like text.
pub unsafe fn do_deparse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::deparse::do_deparse(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// String/vector completion: charmatch, pmatch, strtoi, strtrim
// ---------------------------------------------------------------------------

/// R's `charmatch(x, table)` — character matching.
/// Returns integer index of exact match (1-based), or 0 if no match, or NA if ambiguous.
pub unsafe fn do_charmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let table_arg = CAR(CDR(args));
        let nomatch_arg = CAR(CDR(CDR(args)));
        let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
            NA_INTEGER
        } else {
            real_or_default(nomatch_arg, NA_REAL) as c_int
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let nx = XLENGTH(x_arg);
        let nt = if table_arg.is_null() || table_arg == R_NilValue() {
            0
        } else {
            XLENGTH(table_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        for i in 0..nx {
            let x_is_na = as_character_element_is_na(x_arg, i);
            let x_str = if x_is_na {
                String::new()
            } else {
                elt_to_string(x_arg, i)
            };
            let mut exact_matches = 0usize;
            let mut exact_index = nomatch;
            for j in 0..nt {
                let table_is_na = as_character_element_is_na(table_arg, j);
                let exact = if x_is_na || table_is_na {
                    x_is_na && table_is_na
                } else {
                    elt_to_string(table_arg, j) == x_str
                };
                if exact {
                    exact_matches += 1;
                    exact_index = (j + 1) as c_int;
                }
            }

            if exact_matches == 1 {
                *dst.add(i as usize) = exact_index;
                continue;
            }
            if exact_matches > 1 {
                *dst.add(i as usize) = 0;
                continue;
            }

            let mut partial_matches = 0usize;
            let mut partial_index = nomatch;
            if !x_is_na {
                for j in 0..nt {
                    if as_character_element_is_na(table_arg, j) {
                        continue;
                    }
                    let table_str = elt_to_string(table_arg, j);
                    if table_str.starts_with(&x_str) {
                        partial_matches += 1;
                        partial_index = (j + 1) as c_int;
                    }
                }
            }
            *dst.add(i as usize) = if partial_matches == 1 {
                partial_index
            } else if partial_matches > 1 {
                0
            } else {
                nomatch
            };
        }
        result
    }
}

/// R's `pmatch(x, table, nomatch=NA, duplicates.ok=FALSE)` — partial matching.
/// Returns integer vector of matches (1-based).
pub unsafe fn do_pmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let table_arg = CAR(CDR(args));
        let nomatch_arg = CAR(CDR(CDR(args)));
        let duplicates_arg = CAR(CDR(CDR(CDR(args))));
        let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
            NA_INTEGER
        } else {
            real_or_default(nomatch_arg, NA_REAL as f64) as c_int
        };
        let duplicates_ok = if duplicates_arg.is_null() || duplicates_arg == R_NilValue() {
            false
        } else {
            real_or_default(duplicates_arg, 0.0) != 0.0
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let nx = XLENGTH(x_arg);
        let nt = if table_arg.is_null() || table_arg == R_NilValue() {
            0
        } else {
            XLENGTH(table_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        // Track which table entries are already matched
        let mut used = vec![false; nt as usize];

        for i in 0..nx {
            let x_is_na = as_character_element_is_na(x_arg, i);
            let x_str = if x_is_na {
                String::new()
            } else {
                elt_to_string(x_arg, i)
            };
            let mut best_match: c_int = nomatch;
            if x_is_na {
                for j in 0..nt {
                    if !duplicates_ok && used[j as usize] {
                        continue;
                    }
                    if as_character_element_is_na(table_arg, j) {
                        best_match = (j + 1) as c_int;
                        if !duplicates_ok {
                            used[j as usize] = true;
                        }
                        break;
                    }
                }
                *dst.add(i as usize) = best_match;
                continue;
            }

            if x_str.is_empty() {
                *dst.add(i as usize) = nomatch;
                continue;
            }

            for j in 0..nt {
                if !duplicates_ok && used[j as usize] {
                    continue;
                }
                if as_character_element_is_na(table_arg, j) {
                    continue;
                }
                if elt_to_string(table_arg, j) == x_str {
                    best_match = (j + 1) as c_int;
                    if !duplicates_ok {
                        used[j as usize] = true;
                    }
                    break;
                }
            }

            if best_match == nomatch {
                let mut partial_matches = 0usize;
                let mut partial_index = nomatch;
                for j in 0..nt {
                    if !duplicates_ok && used[j as usize] {
                        continue;
                    }
                    if as_character_element_is_na(table_arg, j) {
                        continue;
                    }
                    let t_str = elt_to_string(table_arg, j);
                    if t_str.starts_with(&x_str) {
                        partial_matches += 1;
                        partial_index = (j + 1) as c_int;
                    }
                }
                if partial_matches == 1 {
                    best_match = partial_index;
                    if !duplicates_ok {
                        used[(partial_index - 1) as usize] = true;
                    }
                }
            }
            *dst.add(i as usize) = best_match;
        }
        result
    }
}

/// R's `strtoi(x, base=10L)` — convert strings to integers.
pub unsafe fn do_strtoi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let base_arg = CAR(CDR(args));
        let base = if base_arg.is_null() || base_arg == R_NilValue() {
            10
        } else {
            real_or_default(base_arg, 10.0) as i32
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let val = i64::from_str_radix(s.trim(), base as u32).unwrap_or(NA_INTEGER as i64);
            *dst.add(i as usize) = if val > c_int::MAX as i64 || val < c_int::MIN as i64 {
                NA_INTEGER
            } else {
                val as c_int
            };
        }
        result
    }
}

/// R's `strtrim(x, width)` — truncate strings to a maximum width.
pub unsafe fn do_strtrim(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        super::vector_text_entry::evaluate(args, rho, super::vector_text_owned::Operation::Trim)
    }
}

// ---------------------------------------------------------------------------
// String operations
// ---------------------------------------------------------------------------
