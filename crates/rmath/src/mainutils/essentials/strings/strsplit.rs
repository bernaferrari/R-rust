//! strsplit and chartr

use super::*;

/// R's `strsplit(x, split, fixed=FALSE, perl=FALSE, useBytes=FALSE)` — split
/// strings by matches of `split`, returning a list of token vectors.
///
/// Mirrors upstream `do_strsplit` (grep.c): every match splits — a
/// non-empty match is dropped and the text before it becomes a token; an
/// empty match consumes the current character, which itself becomes the
/// token. A trailing remainder is kept only when non-empty. The `split`
/// vector recycles across `x`; an empty pattern splits into characters; an
/// NA split token does not split; NA strings pass through as NA.
pub unsafe fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = arg_by_name_or_position(args, &["x"], 0);
        let split_arg = arg_by_name_or_position(args, &["split"], 1);

        if x_arg.is_null() || x_arg == R_NilValue() || split_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let mut perl = named_logical_arg(args, "perl").unwrap_or(false);
        super::shared::ignore_perl_when_fixed(&mut perl, fixed);
        let n = XLENGTH(x_arg);
        let tlen = if split_arg == R_NilValue() {
            0
        } else {
            XLENGTH(split_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            // NA input string passes through as NA.
            if TYPEOF(x_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(x_arg, i as i64) == crate::sexp::globals::R_NaString()
            {
                let na_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                if !na_vec.is_null() {
                    let _na_guard = protect(na_vec);
                    SET_STRING_ELT(na_vec, 0, crate::sexp::globals::R_NaString());
                    SET_VECTOR_ELT(result, i as i64, na_vec);
                }
                continue;
            }
            let s = elt_to_string(x_arg, i);
            // NA split token doesn't split: whole string as the only token.
            let na_split = tlen > 0
                && TYPEOF(split_arg) == SEXPTYPE::STRSXP
                && STRING_ELT(split_arg, (i % tlen) as i64) == crate::sexp::globals::R_NaString();
            let split = if tlen == 0 {
                String::new()
            } else {
                elt_to_string(split_arg, (i % tlen) as i64)
            };

            let tokens: Vec<String> = if na_split {
                vec![s]
            } else if split.is_empty() {
                s.chars().map(|c| c.to_string()).collect()
            } else {
                // Find all matches, then cut tokens around them.
                let mut matches: Vec<(usize, usize)> = Vec::new();
                let mut offset = 0usize;
                if !s.is_empty() {
                    loop {
                        let hay = &s[offset..];
                        let found = if fixed {
                            fixed_find(hay, &split, false)
                        } else if perl {
                            crate::mainutils::grep::perl_find(&split, hay, false)
                        } else {
                            crate::mainutils::grep::ere_find(&split, hay, false)
                        };
                        let Some(m) = found else { break };
                        matches.push((offset + m.start, offset + m.end));
                        if m.end > m.start {
                            offset += m.end;
                        } else {
                            // Empty match gets the next char, so move by one.
                            offset += hay.chars().next().map_or(1, char::len_utf8);
                        }
                        if offset >= s.len() {
                            break;
                        }
                    }
                }
                let mut tokens = Vec::with_capacity(matches.len() + 1);
                let mut pos = 0usize;
                for (st, en) in &matches {
                    if en > st {
                        tokens.push(s[pos..*st].to_string());
                        pos = *en;
                    } else {
                        // Empty match: the current character is the token.
                        let clen = s[*st..].chars().next().map_or(1, char::len_utf8);
                        tokens.push(s[*st..(*st + clen)].to_string());
                        pos = *st + clen;
                    }
                }
                if pos < s.len() {
                    tokens.push(s[pos..].to_string());
                }
                tokens
            };

            let vec = Rf_allocVector3(SEXPTYPE::STRSXP, tokens.len() as R_xlen_t);
            if !vec.is_null() {
                let _vec_guard = protect(vec);
                for (j, part) in tokens.iter().enumerate() {
                    let cstr = CString::new(part.as_str()).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        SET_STRING_ELT(vec, j as i64, charsxp);
                    }
                }
            }
            SET_VECTOR_ELT(result, i as i64, vec);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Conversion: chartr, format
// ---------------------------------------------------------------------------

/// R's `chartr(old, new, x)` — character-by-character translation.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old_arg = CAR(args);
        let new_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        if old_arg.is_null() || new_arg.is_null() {
            return R_NilValue();
        }
        let old_str = elt_to_string(old_arg, 0);
        let new_str = elt_to_string(new_arg, 0);
        let old_chars: Vec<char> = old_str.chars().collect();
        let new_chars: Vec<char> = new_str.chars().collect();
        let n = if x_arg.is_null() || x_arg == R_NilValue() {
            0
        } else {
            XLENGTH(x_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if as_character_element_is_na(x_arg, i) {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let s = elt_to_string(x_arg, i);
            let translated: String = s
                .chars()
                .map(|c| {
                    if let Some(pos) = old_chars.iter().position(|&oc| oc == c) {
                        *new_chars.get(pos).unwrap_or(&c)
                    } else {
                        c
                    }
                })
                .collect();
            let cstr = CString::new(translated).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        result
    }
}
