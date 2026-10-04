//! Graphics package - graphics primitives

mod base;
mod graphics;
mod init;
pub(crate) mod par;
mod par_common;
pub(crate) mod plot;
pub(crate) mod plot3d;
pub(crate) mod stem;
pub(crate) mod text_metrics;
#[allow(dead_code)]
pub(crate) mod xspline;
use crate::sexp::ffi::SEXP;

unsafe extern "C-unwind" fn c_par(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { par::C_par(call, op, crate::sexp::accessors::CDR(args), rho) }
}

#[cfg(feature = "renderplot-device")]
fn renderplot_backend_active() -> bool {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).current_renderplot_backend.is_some()
    })
}

#[cfg(feature = "renderplot-device")]
unsafe fn external_routine_name(args: SEXP) -> String {
    unsafe {
        let name = crate::sexp::accessors::CAR(args);
        if name.is_null()
            || crate::sexp::accessors::TYPEOF(name) != crate::sexp::ffi::SEXPTYPE::STRSXP
            || crate::sexp::accessors::XLENGTH(name) < 1
        {
            return String::new();
        }
        let chars = crate::sexp::accessors::CHAR(crate::sexp::accessors::STRING_ELT(name, 0));
        if chars.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(chars).to_string_lossy().into_owned()
    }
}

/// `.External` passes the routine name as the first cell. Portable graphics
/// reads the arguments that follow it.
#[cfg(feature = "renderplot-device")]
unsafe fn forward_portable(name: &str, args: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::portable_plot::draw_builtin(name, crate::sexp::accessors::CDR(args))
    }
}

/// GNU `plot.xy` passes one xy.coords list, then type, pch, lty, col, bg, cex, lwd.
/// The portable point/line path reads a named `x` list and a `type` string.
#[cfg(feature = "renderplot-device")]
unsafe fn draw_portable_plot_xy(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, CDR, SETTAG};
        use crate::sexp::constructors::Rf_cons;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::symbol::Rf_install;
        let nil = R_NilValue();
        let mut cell = CDR(args);
        let xy = if cell.is_null() || cell == nil {
            nil
        } else {
            CAR(cell)
        };
        cell = if cell.is_null() || cell == nil {
            nil
        } else {
            CDR(cell)
        };
        let typ = if cell.is_null() || cell == nil {
            nil
        } else {
            CAR(cell)
        };
        cell = if cell.is_null() || cell == nil {
            nil
        } else {
            CDR(cell)
        };
        let tags = ["pch", "lty", "col", "bg", "cex", "lwd"];
        let mut extras = Vec::new();
        let mut index = 0;
        while !cell.is_null() && cell != nil && index < tags.len() {
            extras.push((tags[index], CAR(cell)));
            cell = CDR(cell);
            index += 1;
        }
        let mut built = nil;
        let mut guard = None;
        for (tag, value) in extras.into_iter().rev() {
            built = Rf_cons(value, built);
            guard = Some(crate::sexp::protect::protect(built));
            let ctag = std::ffi::CString::new(tag).unwrap_or_default();
            SETTAG(built, Rf_install(ctag.as_ptr()));
        }
        built = Rf_cons(typ, built);
        guard = Some(crate::sexp::protect::protect(built));
        SETTAG(built, Rf_install(c"type".as_ptr()));
        built = Rf_cons(xy, built);
        guard = Some(crate::sexp::protect::protect(built));
        SETTAG(built, Rf_install(c"x".as_ptr()));
        let drawn = crate::mainutils::portable_plot::draw_builtin("points", built);
        drop(guard);
        drawn
    }
}

unsafe extern "C-unwind" fn c_plot_new(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        #[cfg(feature = "renderplot-device")]
        if renderplot_backend_active() {
            // The renderplot scene is the device. The legacy plot.new calls
            // GEcurrentDevice, which opens options("device") and writes pdf.
            return crate::mainutils::portable_plot::draw_builtin(
                "plot.new",
                crate::sexp::accessors::CDR(args),
            );
        }
        plot::C_plot_new(call, op, args, rho)
    }
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

unsafe extern "C-unwind" fn c_plot_xy(args: SEXP) -> SEXP {
    unsafe {
        #[cfg(feature = "renderplot-device")]
        if renderplot_backend_active() {
            let name = external_routine_name(args);
            let bare = name.strip_prefix("C_").unwrap_or(name.as_str());
            return match bare {
                "plotXY" | "plot_xy" => draw_portable_plot_xy(args),
                "box" | "title" | "segments" | "rect" | "polygon" | "abline" => {
                    forward_portable(bare, args)
                }
                _ => crate::sexp::globals::R_NilValue(),
            };
        }
        let _ = args;
        crate::sexp::globals::R_NilValue()
    }
}
unsafe extern "C-unwind" fn c_axis(args: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::essentials::do_axis(
            crate::sexp::globals::R_NilValue(),
            crate::sexp::globals::R_NilValue(),
            // The first cell is the routine name. Side follows it.
            crate::sexp::accessors::CDR(args),
            crate::sexp::globals::R_GlobalEnv(),
        )
    }
}
unsafe extern "C-unwind" fn c_bin_count(x: SEXP, breaks: SEXP, right: SEXP, lowest: SEXP) -> SEXP {
    unsafe { stem::C_BinCount(x, breaks, right, lowest) }
}
unsafe extern "C-unwind" fn c_str_width(args: SEXP) -> SEXP {
    unsafe { text_metrics::measure(args, false) }
}
unsafe extern "C-unwind" fn c_str_height(args: SEXP) -> SEXP {
    unsafe { text_metrics::measure(args, true) }
}
unsafe extern "C-unwind" fn c_stem_leaf(x: SEXP, scale: SEXP, width: SEXP, atom: SEXP) -> SEXP {
    unsafe { stem::C_StemLeaf(x, scale, width, atom) }
}

unsafe extern "C-unwind" fn c_contour_def() -> SEXP {
    unsafe { plot3d::C_contourDef() }
}
unsafe extern "C-unwind" fn c_contour(args: SEXP) -> SEXP {
    unsafe { plot3d::C_contour(args) }
}
unsafe extern "C-unwind" fn c_image(args: SEXP) -> SEXP {
    unsafe { plot3d::C_image(args) }
}
unsafe extern "C-unwind" fn c_layout(args: SEXP) -> SEXP {
    unsafe { par::C_layout(args) }
}
unsafe extern "C-unwind" fn c_nil(_args: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_NilValue() }
}

/// GNU `recordPlot` is `.External2(C_getSnapshot)` in the grDevices namespace.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_get_snapshot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::graphics_recording::record(call, op, args, rho) }
}

/// `.External2(C_playSnapshot, x)` puts the routine name in the first cell.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_play_snapshot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::graphics_recording::replay(
            call,
            op,
            crate::sexp::accessors::CDR(args),
            rho,
        )
    }
}


pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "par" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_par,
        )),
        "plot_new" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_plot_new,
        )),
        #[cfg(feature = "renderplot-device")]
        "getSnapshot" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_get_snapshot,
        )),
        #[cfg(feature = "renderplot-device")]
        "playSnapshot" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_play_snapshot,
        )),
        "plot_window" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_plot_window,
        )),
        "axis" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_axis,
        )),
        "plotXY" | "plot_xy" | "title" | "text" | "mtext" | "box" | "segments" | "rect"
        | "polygon" | "abline" => Some(
            crate::mainutils::native_routines::NativeRoutine::External1(c_plot_xy),
        ),
        "strWidth" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_str_width,
        )),
        "strHeight" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_str_height,
        )),
        "BinCount" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_bin_count),
        )),
        "contourDef" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args0(c_contour_def),
        )),
        "contour" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_contour,
        )),
        "image" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_image,
        )),
        "layout" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_layout,
        )),
        "filledcontour" | "persp" | "arrows" | "clip" | "convertX" | "convertY" | "dend"
        | "dendwindow" | "erase" | "path" | "raster" | "symbols" | "xspline" | "locator"
        | "identify" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_nil,
        )),
        "StemLeaf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_stem_leaf),
        )),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: SEXP) {
    unsafe {
        for name in ["C_par", "C_plot_new", "C_plot_window", "C_plotXY", "C_title", "C_text", "C_mtext", "C_axis", "C_box", "C_segments", "C_rect", "C_polygon", "C_abline", "C_strWidth", "C_strHeight", "C_BinCount", "C_contourDef", "C_contour", "C_image", "C_layout", "C_filledcontour", "C_persp", "C_arrows", "C_clip", "C_convertX", "C_convertY", "C_dend", "C_dendwindow", "C_erase", "C_path", "C_raster", "C_symbols", "C_xspline", "C_locator", "C_identify", "C_StemLeaf",
        ] {
            let cname = std::ffi::CString::new(name).unwrap_or_default();
            crate::sexp::envir::defineVar(
                crate::sexp::symbol::Rf_install(cname.as_ptr()),
                crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
                env,
            );
        }
    }
}
