//! colors — extracted verbatim from colors.rs.

use super::*;

// ---------------------------------------------------------------------------
// Default palette
// ---------------------------------------------------------------------------

pub(crate) const DEFAULT_PALETTE: [rcolor; 8] = [
    0xff000000, 0xff6b53df, 0xff4fd061, 0xffe69722, 0xffe5e228, 0xffbc0bcd, 0xff10c7f5, 0xff9e9e9e,
];

// ---------------------------------------------------------------------------
// Per-session palette state
// ---------------------------------------------------------------------------

pub(crate) const PALETTE_INIT: [rcolor; 32] = [
    0xff000000, 0xff6b53df, 0xff4fd061, 0xffe69722, 0xffe5e228, 0xffbc0bcd, 0xff10c7f5, 0xff9e9e9e,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Save/restore palette (NOT #[unsafe(no_mangle)] — main/colors.rs already exports it)
pub(crate) extern "C" fn savePalette_impl(save: c_int) {
    {
        with_color_state(|state| {
            let ps = state.palette_size as usize;
            if save != 0 {
                state.palette0[..ps].copy_from_slice(&state.palette[..ps]);
            } else {
                state.palette[..ps].copy_from_slice(&state.palette0[..ps]);
            }
        });
    }
}

// ---------------------------------------------------------------------------
// initPalette — register function pointers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// initPalette — register function pointers
// ---------------------------------------------------------------------------

/// Wrapper for inRGBpar3 with void pointer signature (for Rg_set_col_ptrs).
///
/// SAFETY: the engine calls this callback with a live SEXP in `x`.
pub(crate) unsafe extern "C" fn inRGBpar3_dispatch(
    x: *mut std::os::raw::c_void,
    i: c_int,
    bg: c_uint,
) -> c_uint {
    inRGBpar3(x as SEXP, i, bg)
}

pub fn initPalette() {
    crate::main::colors::Rg_set_col_ptrs(
        Some(inRGBpar3_dispatch),
        Some(incol2name),
        Some(inR_GE_str2col),
        Some(savePalette_impl),
    );
}

pub fn do_palette(val: SEXP) -> SEXP {
    {
        if Rf_isString(val) == 0 {
            Rf_error_cstr(c"invalid argument type");
        }

        // Record current palette
        let ps = with_color_state(|state| state.palette_size) as usize;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, ps as c_int);
        let _ans_guard = protect(ans);
        let mut i = 0usize;
        while i < ps {
            let color = with_color_state(|state| state.palette[i]);
            // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
            SET_STRING_ELT(ans, i as R_xlen_t, unsafe {
                mk_char_ptr(incol2name(color))
            });
            i += 1;
        }

        let n = LENGTH(val) as usize;
        if n == 1 {
            // StrMatch ignores spaces and ASCII case.
            let name = charsxp_as_utf8(STRING_ELT(val, 0));
            let matches_default = name
                .iter()
                .filter(|&&b| b != b' ')
                .map(|b| b.to_ascii_lowercase())
                .eq(b"default".iter().copied());
            if matches_default {
                with_color_state(|state| {
                    state.palette[..DEFAULT_PALETTE.len()].copy_from_slice(&DEFAULT_PALETTE);
                    state.palette_size = DEFAULT_PALETTE.len() as c_int;
                });
            } else {
                Rf_error_cstr(c"unknown palette (need >= 2 colors)");
            }
        } else if n > 1 {
            if n > MAX_PALETTE_SIZE {
                Rf_error_cstr(c"maximum number of colors is 1024");
            }
            let mut color_buf: [rcolor; MAX_PALETTE_SIZE] = [0; MAX_PALETTE_SIZE];
            let mut i = 0usize;
            while i < n {
                let name = charsxp_as_utf8(STRING_ELT(val, i as R_xlen_t));
                if name.contains(&0) {
                    crate::sexp::context::r_error("invalid color specification");
                }
                color_buf[i] = if name.first() == Some(&b'#') {
                    rgb2col_bytes(&name)
                } else {
                    name2col_bytes(&name)
                };
                i += 1;
            }
            with_color_state(|state| {
                state.palette[..n].copy_from_slice(&color_buf[..n]);
                state.palette_size = n as c_int;
            });
        }

        ans
    }
}

pub fn do_palette2(val: SEXP) -> SEXP {
    {
        let ps = with_color_state(|state| state.palette_size) as usize;
        let ans = Rf_allocVector(SEXPTYPE::INTSXP, ps as c_int);
        let _ans_guard = protect(ans);
        let mut i = 0usize;
        while i < ps {
            let color = with_color_state(|state| state.palette[i]);
            SET_INTEGER_ELT(ans, i as c_int, color as c_int);
            i += 1;
        }

        let n = LENGTH(val) as usize;
        if n > 0 {
            if TYPEOF(val) != SEXPTYPE::INTSXP {
                Rf_error_cstr(c"requires INTSXP argument");
            }
            if n > MAX_PALETTE_SIZE {
                Rf_error_cstr(c"maximum number of colors is 1024");
            }
            with_color_state(|state| {
                let mut i = 0usize;
                while i < n {
                    state.palette[i] = INTEGER_ELT(val, i as c_int) as rcolor;
                    i += 1;
                }
                state.palette_size = n as c_int;
            });
        }
        ans
    }
}
