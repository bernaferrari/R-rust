//! colors — extracted verbatim from colors.rs.

use super::*;

// ---------------------------------------------------------------------------
// SEXP-callable color functions
// ---------------------------------------------------------------------------

pub fn do_hsv(h: SEXP, s: SEXP, v: SEXP, a: SEXP) -> SEXP {
    {
        let mut r: c_double = 0.0;
        let mut g: c_double = 0.0;
        let mut b: c_double = 0.0;

        let mut guards = Vec::with_capacity(5);
        let h = coerceVector(h, SEXPTYPE::REALSXP.into());
        guards.push(protect(h));
        let s = coerceVector(s, SEXPTYPE::REALSXP.into());
        guards.push(protect(s));
        let v = coerceVector(v, SEXPTYPE::REALSXP.into());
        guards.push(protect(v));
        let a = if Rf_isNull(a) != 0 {
            a
        } else {
            let a = coerceVector(a, SEXPTYPE::REALSXP.into());
            guards.push(protect(a));
            a
        };

        let nh = XLENGTH(h) as usize;
        let ns = XLENGTH(s) as usize;
        let nv = XLENGTH(v) as usize;
        let na = if Rf_isNull(a) != 0 {
            1usize
        } else {
            XLENGTH(a) as usize
        };

        if nh == 0 || ns == 0 || nv == 0 || na == 0 {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        let mut max = nh;
        if max < ns {
            max = ns;
        }
        if max < nv {
            max = nv;
        }
        if max < na {
            max = na;
        }
        let c = Rf_allocVector(SEXPTYPE::STRSXP, max as c_int);
        guards.push(protect(c));

        if Rf_isNull(a) != 0 {
            let mut i = 0usize;
            while i < max {
                let hh = REAL_ELT(h, (i % nh) as c_int);
                let ss = REAL_ELT(s, (i % ns) as c_int);
                let vv = REAL_ELT(v, (i % nv) as c_int);
                if hh < 0.0 || hh > 1.0 || ss < 0.0 || ss > 1.0 || vv < 0.0 || vv > 1.0 {
                    Rf_error(b"invalid hsv color\0".as_ptr() as *const c_char);
                }
                hsv2rgb(hh, ss, vv, &mut r, &mut g, &mut b);
                SET_STRING_ELT(
                    c,
                    i as R_xlen_t,
                    mk_char_ptr(RGB2rgb_func(ScaleColor(r), ScaleColor(g), ScaleColor(b))),
                );
                i += 1;
            }
        } else {
            let mut i = 0usize;
            while i < max {
                let hh = REAL_ELT(h, (i % nh) as c_int);
                let ss = REAL_ELT(s, (i % ns) as c_int);
                let vv = REAL_ELT(v, (i % nv) as c_int);
                let aa = REAL_ELT(a, (i % na) as c_int);
                if hh < 0.0
                    || hh > 1.0
                    || ss < 0.0
                    || ss > 1.0
                    || vv < 0.0
                    || vv > 1.0
                    || aa < 0.0
                    || aa > 1.0
                {
                    Rf_error(b"invalid hsv color\0".as_ptr() as *const c_char);
                }
                hsv2rgb(hh, ss, vv, &mut r, &mut g, &mut b);
                SET_STRING_ELT(
                    c,
                    i as R_xlen_t,
                    mk_char_ptr(RGBA2rgb_func(
                        ScaleColor(r),
                        ScaleColor(g),
                        ScaleColor(b),
                        ScaleAlpha(aa),
                    )),
                );
                i += 1;
            }
        }
        c
    }
}

pub fn do_hcl(h: SEXP, c: SEXP, l: SEXP, a: SEXP, sfixup: SEXP) -> SEXP {
    {
        let fixup = asLogical(sfixup);
        let mut guards = Vec::with_capacity(5);
        let h = coerceVector(h, SEXPTYPE::REALSXP.into());
        guards.push(protect(h));
        let c = coerceVector(c, SEXPTYPE::REALSXP.into());
        guards.push(protect(c));
        let l = coerceVector(l, SEXPTYPE::REALSXP.into());
        guards.push(protect(l));
        let a = if Rf_isNull(a) != 0 {
            a
        } else {
            let a = coerceVector(a, SEXPTYPE::REALSXP.into());
            guards.push(protect(a));
            a
        };

        let nh = XLENGTH(h) as usize;
        let nc = XLENGTH(c) as usize;
        let nl = XLENGTH(l) as usize;
        let na = if Rf_isNull(a) != 0 {
            1usize
        } else {
            XLENGTH(a) as usize
        };

        if nh == 0 || nc == 0 || nl == 0 || na == 0 {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        let mut max = nh;
        if max < nc {
            max = nc;
        }
        if max < nl {
            max = nl;
        }
        if max < na {
            max = na;
        }
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, max as c_int);
        guards.push(protect(ans));

        if Rf_isNull(a) != 0 {
            let mut i = 0usize;
            while i < max {
                let H = REAL_ELT(h, (i % nh) as c_int);
                let C = REAL_ELT(c, (i % nc) as c_int);
                let L = REAL_ELT(l, (i % nl) as c_int);
                if R_FINITE(H) && R_FINITE(C) && R_FINITE(L) {
                    if L < 0.0 || L > WHITE_Y || C < 0.0 {
                        Rf_error(b"invalid hcl color\0".as_ptr() as *const c_char);
                    }
                    let mut rv: c_double = 0.0;
                    let mut gv: c_double = 0.0;
                    let mut bv: c_double = 0.0;
                    hcl2rgb(H, C, L, &mut rv, &mut gv, &mut bv);
                    let mut ir = (255.0 * rv + 0.5) as c_int;
                    let mut ig = (255.0 * gv + 0.5) as c_int;
                    let mut ib = (255.0 * bv + 0.5) as c_int;
                    if FixupColor(&mut ir, &mut ig, &mut ib) != 0 && fixup == 0 {
                        SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                    } else {
                        SET_STRING_ELT(
                            ans,
                            i as R_xlen_t,
                            mk_char_ptr(RGB2rgb_func(ir as u32, ig as u32, ib as u32)),
                        );
                    }
                } else {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                }
                i += 1;
            }
        } else {
            let mut i = 0usize;
            while i < max {
                let H = REAL_ELT(h, (i % nh) as c_int);
                let C = REAL_ELT(c, (i % nc) as c_int);
                let L = REAL_ELT(l, (i % nl) as c_int);
                let mut A = REAL_ELT(a, (i % na) as c_int);
                if !R_FINITE(A) {
                    A = 1.0;
                }
                if R_FINITE(H) && R_FINITE(C) && R_FINITE(L) {
                    if L < 0.0 || L > WHITE_Y || C < 0.0 || A < 0.0 || A > 1.0 {
                        Rf_error(b"invalid hcl color\0".as_ptr() as *const c_char);
                    }
                    let mut rv: c_double = 0.0;
                    let mut gv: c_double = 0.0;
                    let mut bv: c_double = 0.0;
                    hcl2rgb(H, C, L, &mut rv, &mut gv, &mut bv);
                    let mut ir = (255.0 * rv + 0.5) as c_int;
                    let mut ig = (255.0 * gv + 0.5) as c_int;
                    let mut ib = (255.0 * bv + 0.5) as c_int;
                    if FixupColor(&mut ir, &mut ig, &mut ib) != 0 && fixup == 0 {
                        SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                    } else {
                        SET_STRING_ELT(
                            ans,
                            i as R_xlen_t,
                            mk_char_ptr(RGBA2rgb_func(
                                ir as u32,
                                ig as u32,
                                ib as u32,
                                ScaleAlpha(A),
                            )),
                        );
                    }
                } else {
                    SET_STRING_ELT(ans, i as R_xlen_t, NA_STRING());
                }
                i += 1;
            }
        }
        ans
    }
}

pub fn do_rgb(r: SEXP, g: SEXP, b: SEXP, a: SEXP, mcv: SEXP, nam: SEXP) -> SEXP {
    {
        let mV = asReal(mcv);
        if !R_FINITE(mV) || mV == 0.0 {
            Rf_error(b"invalid value of 'maxColorValue'\0".as_ptr() as *const c_char);
        }

        let mut guards = Vec::with_capacity(6);
        let (r, g, b, a) = if mV == 255.0 {
            (
                coerceVector(r, SEXPTYPE::INTSXP.into()),
                coerceVector(g, SEXPTYPE::INTSXP.into()),
                coerceVector(b, SEXPTYPE::INTSXP.into()),
                if Rf_isNull(a) != 0 {
                    a
                } else {
                    coerceVector(a, SEXPTYPE::INTSXP.into())
                },
            )
        } else {
            (
                coerceVector(r, SEXPTYPE::REALSXP.into()),
                coerceVector(g, SEXPTYPE::REALSXP.into()),
                coerceVector(b, SEXPTYPE::REALSXP.into()),
                if Rf_isNull(a) != 0 {
                    a
                } else {
                    coerceVector(a, SEXPTYPE::REALSXP.into())
                },
            )
        };
        guards.push(protect(r));
        guards.push(protect(g));
        guards.push(protect(b));
        if Rf_isNull(a) == 0 {
            guards.push(protect(a));
        }

        let nr = XLENGTH(r) as usize;
        let ng = XLENGTH(g) as usize;
        let nb = XLENGTH(b) as usize;
        let na = if Rf_isNull(a) != 0 {
            1usize
        } else {
            XLENGTH(a) as usize
        };

        if nr == 0 || ng == 0 || nb == 0 || na == 0 {
            return Rf_allocVector(SEXPTYPE::STRSXP, 0);
        }

        let mut l_max = nr;
        if l_max < ng {
            l_max = ng;
        }
        if l_max < nb {
            l_max = nb;
        }
        if l_max < na {
            l_max = na;
        }

        let nam = coerceVector(nam, SEXPTYPE::STRSXP.into());
        guards.push(protect(nam));
        if LENGTH(nam) != 0 && LENGTH(nam) != l_max as c_int {
            Rf_error(b"invalid 'names' vector\0".as_ptr() as *const c_char);
        }
        let c = Rf_allocVector(SEXPTYPE::STRSXP, l_max as c_int);
        guards.push(protect(c));

        if mV == 255.0 {
            if Rf_isNull(a) != 0 {
                let mut i = 0usize;
                while i < l_max {
                    let sr = CheckColor(INTEGER_ELT(r, (i % nr) as c_int));
                    let sg = CheckColor(INTEGER_ELT(g, (i % ng) as c_int));
                    let sb = CheckColor(INTEGER_ELT(b, (i % nb) as c_int));
                    SET_STRING_ELT(c, i as R_xlen_t, mk_char_ptr(RGB2rgb_func(sr, sg, sb)));
                    i += 1;
                }
            } else {
                let mut i = 0usize;
                while i < l_max {
                    let sr = CheckColor(INTEGER_ELT(r, (i % nr) as c_int));
                    let sg = CheckColor(INTEGER_ELT(g, (i % ng) as c_int));
                    let sb = CheckColor(INTEGER_ELT(b, (i % nb) as c_int));
                    let sa = CheckAlpha(INTEGER_ELT(a, (i % na) as c_int));
                    SET_STRING_ELT(c, i as R_xlen_t, mk_char_ptr(RGBA2rgb_func(sr, sg, sb, sa)));
                    i += 1;
                }
            }
        } else if mV == 1.0 {
            if Rf_isNull(a) != 0 {
                let mut i = 0usize;
                while i < l_max {
                    let sr = ScaleColor(REAL_ELT(r, (i % nr) as c_int));
                    let sg = ScaleColor(REAL_ELT(g, (i % ng) as c_int));
                    let sb = ScaleColor(REAL_ELT(b, (i % nb) as c_int));
                    SET_STRING_ELT(c, i as R_xlen_t, mk_char_ptr(RGB2rgb_func(sr, sg, sb)));
                    i += 1;
                }
            } else {
                let mut i = 0usize;
                while i < l_max {
                    let sr = ScaleColor(REAL_ELT(r, (i % nr) as c_int));
                    let sg = ScaleColor(REAL_ELT(g, (i % ng) as c_int));
                    let sb = ScaleColor(REAL_ELT(b, (i % nb) as c_int));
                    let sa = ScaleAlpha(REAL_ELT(a, (i % na) as c_int));
                    SET_STRING_ELT(c, i as R_xlen_t, mk_char_ptr(RGBA2rgb_func(sr, sg, sb, sa)));
                    i += 1;
                }
            }
        } else {
            if Rf_isNull(a) != 0 {
                let mut i = 0usize;
                while i < l_max {
                    let sr = ScaleColor(REAL_ELT(r, (i % nr) as c_int) / mV);
                    let sg = ScaleColor(REAL_ELT(g, (i % ng) as c_int) / mV);
                    let sb = ScaleColor(REAL_ELT(b, (i % nb) as c_int) / mV);
                    SET_STRING_ELT(c, i as R_xlen_t, mk_char_ptr(RGB2rgb_func(sr, sg, sb)));
                    i += 1;
                }
            } else {
                let mut i = 0usize;
                while i < l_max {
                    let sr = ScaleColor(REAL_ELT(r, (i % nr) as c_int) / mV);
                    let sg = ScaleColor(REAL_ELT(g, (i % ng) as c_int) / mV);
                    let sb = ScaleColor(REAL_ELT(b, (i % nb) as c_int) / mV);
                    let sa = ScaleAlpha(REAL_ELT(a, (i % na) as c_int) / mV);
                    SET_STRING_ELT(c, i as R_xlen_t, mk_char_ptr(RGBA2rgb_func(sr, sg, sb, sa)));
                    i += 1;
                }
            }
        }

        if LENGTH(nam) != 0 {
            setAttrib(c, R_NamesSymbol(), nam);
        }
        c
    }
}

pub fn do_gray(lev: SEXP, a: SEXP) -> SEXP {
    {
        let mut guards = Vec::with_capacity(3);
        let lev = coerceVector(lev, SEXPTYPE::REALSXP.into());
        guards.push(protect(lev));
        let nlev = LENGTH(lev) as usize;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, nlev as c_int);
        if nlev == 0 {
            return ans;
        }
        guards.push(protect(ans));
        let a = if Rf_isNull(a) != 0 {
            a
        } else {
            let a = coerceVector(a, SEXPTYPE::REALSXP.into());
            guards.push(protect(a));
            a
        };

        if Rf_isNull(a) != 0 {
            let mut i = 0usize;
            while i < nlev {
                let level = REAL_ELT(lev, i as c_int);
                if ISNAN(level) || level < 0.0 || level > 1.0 {
                    Rf_error(b"invalid gray level, must be in [0,1].\0".as_ptr() as *const c_char);
                }
                let ilevel = (255.0 * level + 0.5) as c_int;
                SET_STRING_ELT(
                    ans,
                    i as R_xlen_t,
                    mk_char_ptr(RGB2rgb_func(ilevel as u32, ilevel as u32, ilevel as u32)),
                );
                i += 1;
            }
        } else {
            let na = LENGTH(a) as usize;
            let max = if nlev > na { nlev } else { na };
            let mut i = 0usize;
            while i < max {
                let level = REAL_ELT(lev, (i % nlev) as c_int);
                if ISNAN(level) || level < 0.0 || level > 1.0 {
                    Rf_error(b"invalid gray level, must be in [0,1].\0".as_ptr() as *const c_char);
                }
                let ilevel = (255.0 * level + 0.5) as c_int;
                let aa = REAL_ELT(a, (i % na) as c_int);
                SET_STRING_ELT(
                    ans,
                    i as R_xlen_t,
                    mk_char_ptr(RGBA2rgb_func(
                        ilevel as u32,
                        ilevel as u32,
                        ilevel as u32,
                        ScaleAlpha(aa),
                    )),
                );
                i += 1;
            }
        }
        ans
    }
}

pub fn do_RGB2hsv(rgb: SEXP) -> SEXP {
    {
        use crate::main::array::allocMatrix;

        let mut guards = Vec::with_capacity(4);
        let rgb = coerceVector(rgb, SEXPTYPE::REALSXP.into());
        guards.push(protect(rgb));
        if !isMatrix(rgb) {
            Rf_error(b"rgb is not a matrix (internally)\0".as_ptr() as *const c_char);
        }
        let dd = getAttrib(rgb, R_DimSymbol());
        if INTEGER_ELT(dd, 0) != 3 {
            Rf_error(b"rgb must have 3 rows (internally)\0".as_ptr() as *const c_char);
        }
        let n = INTEGER_ELT(dd, 1) as usize;

        // SAFETY: allocation runs on the R session thread that owns `rgb`.
        let ans = unsafe { allocMatrix(SEXPTYPE::REALSXP.into(), 3, n as c_int) };
        guards.push(protect(ans));
        let dmns = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        guards.push(protect(dmns));
        let names = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        guards.push(protect(names));
        SET_STRING_ELT(names, 0, mk_char_ptr(b"h\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 1, mk_char_ptr(b"s\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 2, mk_char_ptr(b"v\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(dmns, 0, names);

        if Rf_isNull(getAttrib(rgb, R_DimNamesSymbol())) == 0 {
            let dd2 = getAttrib(rgb, R_DimNamesSymbol());
            if Rf_isNull(dd2) == 0 {
                let col_names = VECTOR_ELT(dd2, 1);
                if Rf_isNull(col_names) == 0 {
                    SET_VECTOR_ELT(dmns, 1, col_names);
                }
            }
        }
        setAttrib(ans, R_DimNamesSymbol(), dmns);

        let mut i = 0usize;
        let mut i3 = 0usize;
        while i < n {
            let mut hh: c_double = 0.0;
            let mut ss: c_double = 0.0;
            let mut vv: c_double = 0.0;
            rgb2hsv(
                REAL_ELT(rgb, i3 as c_int),
                REAL_ELT(rgb, (i3 + 1) as c_int),
                REAL_ELT(rgb, (i3 + 2) as c_int),
                &mut hh,
                &mut ss,
                &mut vv,
            );
            SET_REAL_ELT(ans, i3 as c_int, hh);
            SET_REAL_ELT(ans, (i3 + 1) as c_int, ss);
            SET_REAL_ELT(ans, (i3 + 2) as c_int, vv);
            i += 1;
            i3 += 3;
        }
        ans
    }
}

pub fn do_col2rgb(colors: SEXP, alpha: SEXP) -> SEXP {
    {
        use crate::main::array::allocMatrix;

        let alph = asLogical(alpha);
        if alph == NA_LOGICAL {
            Rf_error(b"invalid 'alpha' value\0".as_ptr() as *const c_char);
        }

        let t = TYPEOF(colors);
        let mut guards = Vec::with_capacity(4);
        let colors = match t {
            tt if tt == SEXPTYPE::INTSXP || tt == SEXPTYPE::STRSXP => colors, // INTSXP or STRSXP
            tt if tt == SEXPTYPE::REALSXP => coerceVector(colors, SEXPTYPE::INTSXP.into()),
            _ => coerceVector(colors, SEXPTYPE::STRSXP.into()),
        };
        guards.push(protect(colors));

        let n = LENGTH(colors) as usize;
        // SAFETY: allocation runs on the R session thread that owns `colors`.
        let ans = unsafe { allocMatrix(SEXPTYPE::INTSXP.into(), 3 + alph, n as c_int) };
        guards.push(protect(ans));
        let dmns = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        guards.push(protect(dmns));
        let names = Rf_allocVector(SEXPTYPE::STRSXP, 3 + alph);
        guards.push(protect(names));
        SET_STRING_ELT(names, 0, mk_char_ptr(b"red\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 1, mk_char_ptr(b"green\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 2, mk_char_ptr(b"blue\0".as_ptr() as *const c_char));
        if alph != 0 {
            SET_STRING_ELT(names, 3, mk_char_ptr(b"alpha\0".as_ptr() as *const c_char));
        }
        SET_VECTOR_ELT(dmns, 0, names);

        let col_names = getAttrib(colors, R_NamesSymbol());
        if Rf_isNull(col_names) == 0 {
            SET_VECTOR_ELT(dmns, 1, col_names);
        }
        setAttrib(ans, R_DimNamesSymbol(), dmns);

        let mut j = 0usize;
        let mut i = 0usize;
        while i < n {
            let icol = inRGBpar3(colors, i as c_int, R_TRANWHITE);
            SET_INTEGER_ELT(ans, j as c_int, R_RED(icol));
            j += 1;
            SET_INTEGER_ELT(ans, j as c_int, R_GREEN(icol));
            j += 1;
            SET_INTEGER_ELT(ans, j as c_int, R_BLUE(icol));
            j += 1;
            if alph != 0 {
                SET_INTEGER_ELT(ans, j as c_int, R_ALPHA(icol));
                j += 1;
            }
            i += 1;
        }
        ans
    }
}

pub fn do_colors() -> SEXP {
    {
        // Count entries
        let mut n = 0usize;
        for entry in COLOR_DATA_BASE.iter() {
            if entry.name.is_empty() {
                break;
            }
            n += 1;
        }
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, n as c_int);
        let _ans_guard = protect(ans);
        let mut i = 0usize;
        for entry in COLOR_DATA_BASE.iter() {
            if entry.name.is_empty() {
                break;
            }
            SET_STRING_ELT(
                ans,
                i as R_xlen_t,
                mk_char_ptr(entry.name.as_ptr() as *const c_char),
            );
            i += 1;
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// Evaluated-builtin wrappers (`(call, op, args, rho)` shape).
//
// The C-ported implementations above take unwrapped arguments
// (`do_col2rgb(colors, alpha)`); the evaluator calls builtins with the
// standard four-argument form, so these thin wrappers unpack `args`.
use crate::sexp::accessors::{CAR, CDR};

pub unsafe fn do_col2rgb_builtin(
    _call: *mut crate::sexp::ffi::SexprecCore,
    _op: *mut crate::sexp::ffi::SexprecCore,
    args: *mut crate::sexp::ffi::SexprecCore,
    _rho: *mut crate::sexp::ffi::SexprecCore,
) -> *mut crate::sexp::ffi::SexprecCore {
    {
        use crate::sexp::ffi::{FALSE, SEXPTYPE};
        use crate::sexp::globals::R_NilValue;
        let colors = CAR(args);
        let alpha_cell = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        // Upstream col2rgb(colors, alpha=FALSE): a missing alpha defaults
        // to FALSE, not NA (which errors as invalid).
        let alpha = if alpha_cell.is_null() || alpha_cell == R_NilValue() {
            crate::sexp::constructors::Rf_ScalarLogical(FALSE)
        } else {
            alpha_cell
        };
        let _guard = crate::sexp::protect::protect(alpha);
        do_col2rgb(colors, alpha)
    }
}

pub unsafe fn do_colors_builtin(
    _call: *mut crate::sexp::ffi::SexprecCore,
    _op: *mut crate::sexp::ffi::SexprecCore,
    _args: *mut crate::sexp::ffi::SexprecCore,
    _rho: *mut crate::sexp::ffi::SexprecCore,
) -> *mut crate::sexp::ffi::SexprecCore {
    do_colors()
}

pub unsafe fn do_palette_builtin(
    _call: *mut crate::sexp::ffi::SexprecCore,
    _op: *mut crate::sexp::ffi::SexprecCore,
    args: *mut crate::sexp::ffi::SexprecCore,
    _rho: *mut crate::sexp::ffi::SexprecCore,
) -> *mut crate::sexp::ffi::SexprecCore {
    {
        // `palette()` with no argument queries the current palette:
        // pass an empty string vector (length 0 hits neither the
        // reset nor the set branch, returning the recorded palette).
        let val = CAR(args);
        let val = if val.is_null() || val == R_NilValue() {
            Rf_allocVector(SEXPTYPE::STRSXP, 0)
        } else {
            val
        };
        let _guard = crate::sexp::protect::protect(val);
        do_palette(val)
    }
}

pub unsafe fn do_rgb_builtin(
    _call: *mut crate::sexp::ffi::SexprecCore,
    _op: *mut crate::sexp::ffi::SexprecCore,
    args: *mut crate::sexp::ffi::SexprecCore,
    _rho: *mut crate::sexp::ffi::SexprecCore,
) -> *mut crate::sexp::ffi::SexprecCore {
    {
        let tag_is = |cell: SEXP, name: &[u8]| -> bool {
            let tag = crate::sexp::accessors::TAG(cell);
            if tag.is_null() || crate::sexp::accessors::TYPEOF(tag) != SEXPTYPE::SYMSXP {
                return false;
            }
            let pname = crate::sexp::accessors::PRINTNAME(tag);
            if pname.is_null() {
                return false;
            }
            crate::sexp::accessors::charsxp_as_utf8(pname).as_slice() == name
        };
        let missing = |value: SEXP| -> bool {
            value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_MissingArg()
        };
        let mut red = R_NilValue();
        let mut green = R_NilValue();
        let mut blue = R_NilValue();
        let mut alpha = R_NilValue();
        let mut names = R_NilValue();
        let mut mcv = R_NilValue();
        let mut positional = 0usize;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            if tag_is(cell, b"red") {
                red = value;
            } else if tag_is(cell, b"green") {
                green = value;
            } else if tag_is(cell, b"blue") {
                blue = value;
            } else if tag_is(cell, b"alpha") {
                alpha = value;
            } else if tag_is(cell, b"names") {
                names = value;
            } else if tag_is(cell, b"maxColorValue") {
                mcv = value;
            } else {
                match positional {
                    0 => red = value,
                    1 => green = value,
                    2 => blue = value,
                    3 => alpha = value,
                    4 => names = value,
                    5 => mcv = value,
                    _ => {}
                }
                positional += 1;
            }
            cell = CDR(cell);
        }
        if missing(mcv)
            || (SEXPTYPE::REALSXP != TYPEOF(mcv)
                && SEXPTYPE::INTSXP != TYPEOF(mcv)
                && SEXPTYPE::LGLSXP != TYPEOF(mcv))
        {
            mcv = crate::sexp::constructors::Rf_ScalarReal(1.0);
        }
        if missing(alpha) {
            alpha = R_NilValue();
        }
        if missing(names) {
            names = R_NilValue();
        }
        let _mcv_guard = crate::sexp::protect::protect(mcv);
        do_rgb(red, green, blue, alpha, mcv, names)
    }
}

pub unsafe fn do_hsv_builtin(
    _call: *mut crate::sexp::ffi::SexprecCore,
    _op: *mut crate::sexp::ffi::SexprecCore,
    args: *mut crate::sexp::ffi::SexprecCore,
    _rho: *mut crate::sexp::ffi::SexprecCore,
) -> *mut crate::sexp::ffi::SexprecCore {
    {
        let h = CAR(args);
        let s = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        // do_hsv(h, s, v, a): map the call's (h, s, v) positionally;
        // alpha defaults to missing (fully opaque).
        let v = if CDR(CDR(args)).is_null() || CDR(CDR(args)) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };
        do_hsv(h, s, v, R_NilValue())
    }
}

pub unsafe fn do_gray_builtin(
    _call: *mut crate::sexp::ffi::SexprecCore,
    _op: *mut crate::sexp::ffi::SexprecCore,
    args: *mut crate::sexp::ffi::SexprecCore,
    _rho: *mut crate::sexp::ffi::SexprecCore,
) -> *mut crate::sexp::ffi::SexprecCore {
    {
        let lev = CAR(args);
        let a = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        do_gray(lev, a)
    }
}
