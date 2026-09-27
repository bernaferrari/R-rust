//! Graphics package - graphics primitives

mod base;
mod graphics;
mod init;
pub(crate) mod par;
mod par_common;
pub(crate) mod plot;
pub(crate) mod plot3d;
pub(crate) mod stem;
#[allow(dead_code)]
pub(crate) mod xspline;
use crate::sexp::ffi::SEXP;

unsafe extern "C-unwind" fn c_par(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { par::C_par(call, op, crate::sexp::accessors::CDR(args), rho) }
}

unsafe extern "C-unwind" fn c_plot_new(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { plot::C_plot_new(call, op, args, rho) }
}
unsafe extern "C-unwind" fn c_plot_window(args: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::essentials::do_plot_window(
            crate::sexp::globals::R_NilValue(),
            crate::sexp::globals::R_NilValue(),
            crate::sexp::accessors::CDR(args),
            crate::sexp::globals::R_GlobalEnv(),
        )
    }
}

unsafe extern "C-unwind" fn c_plot_xy(_args: SEXP) -> SEXP {
    crate::sexp::globals::R_NilValue()
}
unsafe extern "C-unwind" fn c_axis(args: SEXP) -> SEXP {
    unsafe {
        let side_arg = crate::mainutils::essentials::arg_by_name_or_position(args, &["side"], 0);
        let at_arg = crate::mainutils::essentials::arg_by_name_or_position(args, &["at"], 1);
        if !at_arg.is_null()
            && at_arg != crate::sexp::globals::R_NilValue()
            && crate::sexp::accessors::TYPEOF(at_arg) == crate::sexp::ffi::SEXPTYPE::REALSXP
            && crate::sexp::accessors::XLENGTH(at_arg) > 0
        {
            return at_arg;
        }
        let side = if side_arg.is_null() || side_arg == crate::sexp::globals::R_NilValue() {
            1
        } else if crate::sexp::accessors::TYPEOF(side_arg) == crate::sexp::ffi::SEXPTYPE::INTSXP {
            *crate::sexp::accessors::INTEGER(side_arg)
        } else if crate::sexp::accessors::TYPEOF(side_arg) == crate::sexp::ffi::SEXPTYPE::REALSXP {
            *crate::sexp::accessors::REAL(side_arg) as i32
        } else {
            1
        };
        let name = if side == 2 || side == 4 { "yaxp" } else { "xaxp" };
        let axp = match par::parameter(name) {
            par::ParValue::Real(v) if v.len() >= 3 => v,
            _ => return crate::sexp::globals::R_NilValue(),
        };
        let lo = axp[0];
        let hi = axp[1];
        let n = axp[2].round().max(1.0) as i32;
        let step = (hi - lo) / f64::from(n);
        let out = crate::sexp::constructors::Rf_allocVector(
            crate::sexp::ffi::SEXPTYPE::REALSXP,
            n + 1,
        );
        for i in 0..=n {
            *crate::sexp::accessors::REAL(out).add(i as usize) = lo + step * f64::from(i);
        }
        out
    }
}
unsafe extern "C-unwind" fn c_bin_count(x: SEXP, breaks: SEXP, right: SEXP, lowest: SEXP) -> SEXP {
    unsafe { stem::C_BinCount(x, breaks, right, lowest) }
}
unsafe extern "C-unwind" fn c_str_metric(args: SEXP) -> SEXP {
    unsafe {
        let mut cell = crate::sexp::accessors::CDR(args);
        let mut labels = crate::sexp::globals::R_NilValue();
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let value = crate::sexp::accessors::CAR(cell);
            if crate::sexp::accessors::TYPEOF(value) == crate::sexp::ffi::SEXPTYPE::STRSXP {
                labels = value;
                break;
            }
            cell = crate::sexp::accessors::CDR(cell);
        }
        let n = if labels.is_null() || labels == crate::sexp::globals::R_NilValue() {
            0
        } else {
            crate::sexp::accessors::XLENGTH(labels)
        };
        let out = crate::sexp::constructors::Rf_allocVector(crate::sexp::ffi::SEXPTYPE::REALSXP, n as i32);
        for i in 0..n {
            *crate::sexp::accessors::REAL(out).add(i as usize) = 1.0;
        }
        out
    }
}
unsafe extern "C-unwind" fn c_contour_def() -> SEXP {
    unsafe { plot3d::C_contourDef() }
}
unsafe extern "C-unwind" fn c_contour(args: SEXP) -> SEXP {
    unsafe { plot3d::C_contour(args) }
}


pub(crate) fn lookup(name: &str) -> crate::unix::dynload::DL_FUNC {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "par" => Some(unsafe { std::mem::transmute(c_par as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP) }),
        "plot_new" => Some(unsafe { std::mem::transmute(c_plot_new as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP) }),
        "plot_window" => Some(unsafe { std::mem::transmute(c_plot_window as unsafe extern "C-unwind" fn(SEXP) -> SEXP) }),
        "axis" => Some(unsafe { std::mem::transmute(c_axis as unsafe extern "C-unwind" fn(SEXP) -> SEXP) }),
        "plotXY" | "plot_xy" | "title" | "text" | "mtext" | "box" | "segments" | "rect" | "polygon" => {
            Some(unsafe { std::mem::transmute(c_plot_xy as unsafe extern "C-unwind" fn(SEXP) -> SEXP) })
        }
        "strWidth" | "strHeight" => {
            Some(unsafe { std::mem::transmute(c_str_metric as unsafe extern "C-unwind" fn(SEXP) -> SEXP) })
        }
        "BinCount" => Some(unsafe {
            std::mem::transmute(
                c_bin_count as unsafe extern "C-unwind" fn(SEXP, SEXP, SEXP, SEXP) -> SEXP,
            )
        }),
        "contourDef" => Some(unsafe {
            std::mem::transmute(c_contour_def as unsafe extern "C-unwind" fn() -> SEXP)
        }),
        "contour" => Some(unsafe {
            std::mem::transmute(c_contour as unsafe extern "C-unwind" fn(SEXP) -> SEXP)
        }),

        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: SEXP) {
    unsafe {
        for name in ["C_par", "C_plot_new", "C_plot_window", "C_plotXY", "C_title", "C_text", "C_mtext", "C_axis", "C_box", "C_segments", "C_rect", "C_polygon", "C_strWidth", "C_strHeight", "C_BinCount", "C_contourDef", "C_contour"] {
            let cname = std::ffi::CString::new(name).unwrap_or_default();
            crate::sexp::envir::defineVar(
                crate::sexp::symbol::Rf_install(cname.as_ptr()),
                crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
                env,
            );
        }
    }
}
