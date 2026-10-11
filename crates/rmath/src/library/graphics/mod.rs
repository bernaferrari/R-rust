//! Graphics package - graphics primitives

mod base;
pub(crate) mod graphics;
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
        std::ffi::CStr::from_ptr(chars)
            .to_string_lossy()
            .into_owned()
    }
}

/// Device-less profile: GNU's drawing primitives auto-open a device and
/// require an established plot frame (`GCheckState`). Without a renderplot
/// backend there is nothing to draw on, so a drawing call degrades to
/// enforcing exactly that contract and returning `NULL`.
#[cfg(not(feature = "renderplot-device"))]
unsafe fn null_device_draw() -> SEXP {
    unsafe {
        if !graphics::plot_new_called() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "plot.new has not been called yet",
            );
        }
        crate::sexp::globals::R_NilValue()
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

/// Native title arguments are positional in GNU's main/sub/xlab/ylab order.
/// Decode them before allocating; retain the original owner and tail so a
/// collecting callback cannot discard the labels or redirect their authority.
#[cfg(feature = "renderplot-device")]
unsafe fn draw_portable_title(args: SEXP) -> SEXP {
    unsafe {
        draw_portable_positional(
            "title",
            args,
            &["main", "sub", "xlab", "ylab", "line", "outer"],
        )
    }
}

/// Retag GNU native positional arguments before the ordinary portable draw
/// path decodes them, retaining the original argument graph over allocations.
#[cfg(feature = "renderplot-device")]
unsafe fn draw_portable_positional(name: &str, args: SEXP, formals: &[&str]) -> SEXP {
    unsafe {
        let result = (|| {
            use crate::sexp::object::{SessionNodeFactory, SexpResult};
            let owner = crate::sexp::owner::OwnerToken::current()?;
            let _pin = owner.pin()?;
            let factory = SessionNodeFactory::new(owner);
            let input = owner.sexp(args)?.into_owned()?;
            let mut cursor = input.try_cdr()?.into_owned()?;
            let mut labels = Vec::new();
            for name in formals {
                if cursor.is_nil() {
                    break;
                }
                labels.push((name, cursor.try_car()?.into_owned()?));
                cursor = cursor.try_cdr()?.into_owned()?;
            }
            let mut built = cursor;
            for (name, value) in labels.into_iter().rev() {
                owner.require_active()?;
                let name = std::ffi::CString::new(*name).expect("static graphics formal");
                let symbol = crate::sexp::symbol::Rf_install(name.as_ptr());
                owner.require_active()?;
                let tag = owner.sexp(symbol)?.into_owned()?;
                built = factory.pairlist_cell(&value, &built, &tag)?.into_owned()?;
            }
            owner.require_active()?;
            let drawn = if name == "rasterImage" {
                crate::mainutils::portable_plot::native_raster_image(built.as_raw())
            } else {
                crate::mainutils::portable_plot::draw_builtin(name, built.as_raw())
            };
            owner.require_active()?;
            SexpResult::Ok(drawn)
        })();
        result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    }
}

/// GNU text.default passes one xy.coords list before labels and text options.
/// Expand its original owned x/y values before the common positional adapter.
#[cfg(feature = "renderplot-device")]
unsafe fn draw_portable_text(args: SEXP) -> SEXP {
    unsafe {
        let result = (|| {
            use crate::sexp::object::{SessionNodeFactory, SexpError, SexpResult};
            let owner = crate::sexp::owner::OwnerToken::current()?;
            let _pin = owner.pin()?;
            let factory = SessionNodeFactory::new(owner);
            let input = owner.sexp(args)?.into_owned()?;
            let payload = input.try_cdr()?.into_owned()?;
            let xy = payload.try_car()?.into_owned()?;
            if xy.typeof_() != crate::sexp::ffi::SEXPTYPE::VECSXP || xy.len() < 2 {
                return Err(SexpError::EvaluationFailed {
                    message: "C_text requires an xy.coords list".into(),
                });
            }
            let x = xy.try_vector_elt(0)?.into_owned()?;
            let y = xy.try_vector_elt(1)?.into_owned()?;
            let tail = payload.try_cdr()?.into_owned()?;
            let nil = factory.nil().into_owned()?;
            let tail = factory.pairlist_cell(&y, &tail, &nil)?.into_owned()?;
            let tail = factory.pairlist_cell(&x, &tail, &nil)?.into_owned()?;
            let expanded = factory.pairlist_cell(&nil, &tail, &nil)?.into_owned()?;
            owner.require_active()?;
            let drawn = draw_portable_positional(
                "text",
                expanded.as_raw(),
                &[
                    "x", "y", "labels", "adj", "pos", "offset", "vfont", "cex", "col", "font",
                ],
            );
            owner.require_active()?;
            SexpResult::Ok(drawn)
        })();
        result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
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
            // GNewPlot records the frame exactly like the device-less path.
            graphics::set_plot_new_state();
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

#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_plot_xy(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            let name = external_routine_name(args);
            let bare = name.strip_prefix("C_").unwrap_or(name.as_str());
            return match bare {
                "plotXY" | "plot_xy" => draw_portable_plot_xy(args),
                "title" => draw_portable_title(args),
                "text" => draw_portable_text(args),
                "mtext" => draw_portable_positional(
                    "mtext",
                    args,
                    &[
                        "text", "side", "line", "outer", "at", "adj", "padj", "cex", "col", "font",
                    ],
                ),
                "polygon" => {
                    draw_portable_positional("polygon", args, &["x", "y", "col", "border", "lty"])
                }
                "abline" => draw_portable_positional(
                    "abline",
                    args,
                    &["a", "b", "h", "v", "untf", "col", "lty", "lwd"],
                ),
                "box" | "segments" | "arrows" | "rect" => forward_portable(bare, args),
                _ => c_unsupported_graphics(args),
            };
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

/// GNU's C_plotXY/C_title/C_text/C_mtext/C_box/C_segments/C_rect/C_polygon/
/// C_abline/C_arrows all start with `GCheckState`. Without a backend the
/// contract is the plot frame itself.
#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_plot_xy(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `clip` passes four user coordinates after the routine name.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_clip(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return crate::mainutils::portable_plot::native_clip(crate::sexp::accessors::CDR(args));
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

/// GNU C_clip converts and stores a clip rectangle; it does not require an
/// established plot frame, so the device-less form only discards the box.
#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_clip(_args: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_NilValue() }
}

/// GNU `grconvertX` passes the values plus one-based unit indices.
unsafe extern "C-unwind" fn c_convert_x(args: SEXP) -> SEXP {
    unsafe { convert_locations(args, false) }
}

/// GNU `grconvertY` passes the values plus one-based unit indices.
unsafe extern "C-unwind" fn c_convert_y(args: SEXP) -> SEXP {
    unsafe { convert_locations(args, true) }
}

#[cfg(feature = "renderplot-device")]
unsafe fn convert_locations(args: SEXP, vertical: bool) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return crate::mainutils::portable_plot::native_convert(
                crate::sexp::accessors::CDR(args),
                vertical,
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

#[cfg(not(feature = "renderplot-device"))]
unsafe fn convert_locations(args: SEXP, _vertical: bool) -> SEXP {
    unsafe {
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
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
unsafe extern "C-unwind" fn c_unsupported_graphics(args: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' is not implemented",
            external_routine_name(args),
        ))
    }
}

#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_raster(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return draw_portable_positional(
                "rasterImage",
                args,
                &[
                    "image",
                    "xleft",
                    "ybottom",
                    "xright",
                    "ytop",
                    "angle",
                    "interpolate",
                ],
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

/// GNU C_raster requires a plot frame (`GCheckState`) before blitting.
#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_raster(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `persp` — validation and the viewing transformation live in the
/// plot3d port; the drawing half runs only with a live renderplot backend,
/// so the device-less profile matches GNU's null device (matrix, no draw).
unsafe extern "C-unwind" fn c_persp(args: SEXP) -> SEXP {
    unsafe { plot3d::C_persp(args) }
}

/// GNU `filledcontour` — plot.new is required (GCheckState); the colored
/// bands are drawn through the portable engine when a backend is live.
unsafe extern "C-unwind" fn c_filledcontour(args: SEXP) -> SEXP {
    unsafe { plot3d::C_filledcontour(args) }
}

/// GNU `path` (polypath) — requires a plot frame.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_path(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return draw_portable_positional(
                "path",
                args,
                &["x", "y", "nper", "rule", "col", "border", "lty"],
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_path(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `symbols` — requires a plot frame.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_symbols(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return draw_portable_positional(
                "symbols",
                args,
                &["x", "y", "type", "data", "inches", "bg", "fg"],
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_symbols(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `xspline` — requires a plot frame.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_xspline(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return draw_portable_positional(
                "xspline",
                args,
                &["x", "y", "s", "open", "repEnds", "draw", "col", "border"],
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_xspline(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `erase` — no plot frame required; the transparent default makes the
/// device-less form a silent no-op.
unsafe extern "C-unwind" fn c_erase(args: SEXP) -> SEXP {
    unsafe {
        #[cfg(feature = "renderplot-device")]
        {
            if renderplot_backend_active() {
                return draw_portable_positional("erase", args, &["col"]);
            }
        }
        crate::sexp::globals::R_NilValue()
    }
}

/// GNU `dend` — requires a plot frame.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_dend(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return draw_portable_positional(
                "dend",
                args,
                &["n", "merge", "height", "ord", "hang", "labels"],
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_dend(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `dendwindow` — requires a plot frame.
#[cfg(feature = "renderplot-device")]
unsafe extern "C-unwind" fn c_dendwindow(args: SEXP) -> SEXP {
    unsafe {
        if renderplot_backend_active() {
            return draw_portable_positional(
                "dendwindow",
                args,
                &["n", "merge", "height", "hang", "labels"],
            );
        }
        crate::mainutils::essentials::base_error(format!(
            "native graphics operation '{}' requires an active renderplot device",
            external_routine_name(args),
        ))
    }
}

#[cfg(not(feature = "renderplot-device"))]
unsafe extern "C-unwind" fn c_dendwindow(_args: SEXP) -> SEXP {
    unsafe { null_device_draw() }
}

/// GNU `locator` — `.External2(C_locator, n, type)`. The scene device has
/// no locator callback, so GLocator fails on the first click: the wrapper
/// observes zero points and `locator()` returns NULL.
unsafe extern "C-unwind" fn c_locator(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        #[cfg(feature = "renderplot-device")]
        {
            if renderplot_backend_active() {
                return draw_portable_positional("locator", args, &["n", "type"]);
            }
        }
        null_device_locator(args)
    }
}

/// Device-less locator: enforce the plot frame, then report zero clicks.
unsafe fn null_device_locator(args: SEXP) -> SEXP {
    unsafe {
        if !graphics::plot_new_called() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "plot.new has not been called yet",
            );
        }
        locator_result(crate::sexp::accessors::CDR(args))
    }
}

/// GNU `identify` — `.External2(C_identify, ...)`. Zero clicks identifies
/// nothing: the all-zero result reduces to integer(0) in the wrapper.
unsafe extern "C-unwind" fn c_identify(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        #[cfg(feature = "renderplot-device")]
        {
            if renderplot_backend_active() {
                return draw_portable_positional(
                    "identify",
                    args,
                    &["x", "y", "labels", "n", "plot", "offset", "tol", "atpen"],
                );
            }
        }
        null_device_identify(args)
    }
}

/// Device-less identify: enforce the plot frame, then report zero picks.
unsafe fn null_device_identify(args: SEXP) -> SEXP {
    unsafe {
        if !graphics::plot_new_called() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "plot.new has not been called yet",
            );
        }
        identify_result(crate::sexp::accessors::CDR(args))
    }
}

/// Build GNU's zero-click locator result: NA coordinate vectors of length
/// `n` and a zero observation count.
pub(crate) unsafe fn locator_result(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::*;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{NA_INTEGER, NA_REAL, R_xlen_t, SEXPTYPE};
        use crate::sexp::protect::protect;
        let n = crate::main::coerce::asInteger(arg_or_nil(args, 0));
        if n <= 0 || n == NA_INTEGER {
            crate::mainutils::essentials::base_error("invalid number of points in locator()");
        }
        let type_cell = arg_or_nil(args, 1);
        let kind = if type_cell == crate::sexp::globals::R_NilValue() {
            String::new()
        } else {
            crate::mainutils::essentials::elt_to_string(type_cell, 0)
        };
        if kind.chars().count() != 1 || !"pln".contains(kind.as_str()) {
            crate::mainutils::essentials::base_error("invalid plot type");
        }
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
        let _x = protect(x);
        let y = Rf_allocVector3(SEXPTYPE::REALSXP, n as R_xlen_t);
        let _y = protect(y);
        for i in 0..n as usize {
            *REAL(x).add(i) = NA_REAL;
            *REAL(y).add(i) = NA_REAL;
        }
        let nobs = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        let _nobs = protect(nobs);
        SET_INTEGER_ELT(nobs, 0, 0);
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _ans = protect(ans);
        SET_VECTOR_ELT(ans, 0, x);
        SET_VECTOR_ELT(ans, 1, y);
        SET_VECTOR_ELT(ans, 2, nobs);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        let _names = protect(names);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"x\0".as_ptr().cast()));
        SET_STRING_ELT(names, 1, Rf_mkChar(b"y\0".as_ptr().cast()));
        SET_STRING_ELT(names, 2, Rf_mkChar(b"n\0".as_ptr().cast()));
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), names);
        ans
    }
}

/// Positional helper over a plain argument list.
unsafe fn arg_or_nil(args: SEXP, index: usize) -> SEXP {
    unsafe {
        let mut cell = args;
        let mut i = 0;
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            if i == index {
                return crate::sexp::accessors::CAR(cell);
            }
            cell = crate::sexp::accessors::CDR(cell);
            i += 1;
        }
        crate::sexp::globals::R_NilValue()
    }
}

/// Build GNU's zero-click identify result: all-false `ind` with zero
/// `pos`/`order`.
pub(crate) unsafe fn identify_result(args: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::*;
        use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
        use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXPTYPE};
        use crate::sexp::protect::protect;
        let x = arg_or_nil(args, 0);
        let y = arg_or_nil(args, 1);
        let labels = arg_or_nil(args, 2);
        let npts = crate::main::coerce::asInteger(arg_or_nil(args, 3));
        let plot = crate::main::coerce::asLogical(arg_or_nil(args, 4));
        let offset = arg_or_nil(args, 5);
        let tol = crate::main::coerce::asReal(arg_or_nil(args, 6));
        let atpen = crate::main::coerce::asLogical(arg_or_nil(args, 7));
        if !matches!(SEXPTYPE(TYPEOF(x)), SEXPTYPE::REALSXP | SEXPTYPE::INTSXP)
            || !matches!(SEXPTYPE(TYPEOF(y)), SEXPTYPE::REALSXP | SEXPTYPE::INTSXP)
            || SEXPTYPE(TYPEOF(labels)) != SEXPTYPE::STRSXP
            || !matches!(
                SEXPTYPE(TYPEOF(offset)),
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP
            )
        {
            crate::mainutils::essentials::base_error("incorrect argument type");
        }
        if npts <= 0 || npts == NA_INTEGER {
            crate::mainutils::essentials::base_error("invalid number of points in identify()");
        }
        if plot == NA_LOGICAL {
            crate::mainutils::essentials::base_error("invalid 'plot' value");
        }
        if tol <= 0. || tol.is_nan() {
            crate::mainutils::essentials::base_error("invalid 'tolerance' value");
        }
        if atpen == NA_LOGICAL {
            crate::mainutils::essentials::base_error("invalid 'atpen' value");
        }
        let n = XLENGTH(x) as usize;
        if n != XLENGTH(y) as usize {
            crate::mainutils::essentials::base_error("different argument lengths");
        }
        if XLENGTH(labels) == 0 {
            crate::mainutils::essentials::base_error("zero-length 'labels' specified");
        }
        let ind = Rf_allocVector3(SEXPTYPE::LGLSXP, n as R_xlen_t);
        let _ind = protect(ind);
        let pos = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
        let _pos = protect(pos);
        let order = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
        let _order = protect(order);
        for i in 0..n as i32 {
            SET_LOGICAL_ELT(ind, i, 0);
            SET_INTEGER_ELT(pos, i, 0);
            SET_INTEGER_ELT(order, i, 0);
        }
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _ans = protect(ans);
        SET_VECTOR_ELT(ans, 0, ind);
        SET_VECTOR_ELT(ans, 1, pos);
        SET_VECTOR_ELT(ans, 2, order);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        let _names = protect(names);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"ind\0".as_ptr().cast()));
        SET_STRING_ELT(names, 1, Rf_mkChar(b"pos\0".as_ptr().cast()));
        SET_STRING_ELT(names, 2, Rf_mkChar(b"order\0".as_ptr().cast()));
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), names);
        ans
    }
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
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "plot_new" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_plot_new,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        #[cfg(feature = "renderplot-device")]
        "getSnapshot" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_get_snapshot,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        #[cfg(feature = "renderplot-device")]
        "playSnapshot" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_play_snapshot,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "plot_window" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_plot_window,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "axis" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_axis,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "plotXY" | "plot_xy" | "title" | "text" | "mtext" | "box" | "segments" | "rect"
        | "polygon" | "abline" | "arrows" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External1(
                c_plot_xy,
                crate::mainutils::native_routines::PayloadArity::Variadic,
            ))
        }
        "strWidth" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_str_width,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "strHeight" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_str_height,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "BinCount" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_bin_count),
        )),
        "contourDef" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args0(c_contour_def),
        )),
        "contour" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_contour,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "image" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_image,
            crate::mainutils::native_routines::PayloadArity::Fixed(4),
        )),
        "layout" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_layout,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "filledcontour" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_filledcontour,
            crate::mainutils::native_routines::PayloadArity::Fixed(5),
        )),
        "clip" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_clip,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "convertX" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_convert_x,
            crate::mainutils::native_routines::PayloadArity::Fixed(3),
        )),
        "convertY" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_convert_y,
            crate::mainutils::native_routines::PayloadArity::Fixed(3),
        )),
        "raster" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_raster,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "persp" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_persp,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "path" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_path,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "symbols" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_symbols,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "xspline" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_xspline,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "dend" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_dend,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "dendwindow" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_dendwindow,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "erase" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_erase,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "locator" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_locator,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "identify" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_identify,
            crate::mainutils::native_routines::PayloadArity::Variadic,
        )),
        "StemLeaf" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_stem_leaf),
        )),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: SEXP) {
    unsafe {
        for name in [
            "C_par",
            "C_plot_new",
            "C_plot_window",
            "C_plotXY",
            "C_title",
            "C_text",
            "C_mtext",
            "C_axis",
            "C_box",
            "C_segments",
            "C_rect",
            "C_polygon",
            "C_abline",
            "C_strWidth",
            "C_strHeight",
            "C_BinCount",
            "C_contourDef",
            "C_contour",
            "C_image",
            "C_layout",
            "C_filledcontour",
            "C_persp",
            "C_arrows",
            "C_clip",
            "C_convertX",
            "C_convertY",
            "C_dend",
            "C_dendwindow",
            "C_erase",
            "C_path",
            "C_raster",
            "C_symbols",
            "C_xspline",
            "C_locator",
            "C_identify",
            "C_StemLeaf",
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

#[cfg(all(test, feature = "renderplot-device"))]
mod portable_title_tests {
    use crate::sexp::{ffi::SEXPTYPE, object::SessionNodeFactory, session::RSession};

    #[test]
    fn positional_margin_labels_survive_collecting_allocation() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let mut input = factory.nil().into_owned().unwrap();
        // Reversed native fields: font, col, cex, padj, adj, at, outer,
        // line, side, text, routine. No independent label or tail root remains.
        for index in (0..11).rev() {
            let value = match index {
                0 => factory.strings(&["C_mtext"]).unwrap(),
                1 => factory.strings(&["first label", "second label"]).unwrap(),
                9 | 10 => factory.nil(),
                _ => {
                    let number = match index {
                        2 | 8 => 1.,
                        3 | 4 => 0.,
                        _ => f64::NAN,
                    };
                    // SAFETY: the active test owner is retained and no callback
                    // occurs between publication and the owning wrap.
                    factory
                        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarReal(number) })
                        .unwrap()
                }
            };
            input = factory
                .pairlist_cell(&value, &input, &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap();
        }
        let fired = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = fired.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(true);
            crate::sexp::instance::with_required_current_instance(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 0;
            });
            crate::sexp::gengc::full_gc();
        }));
        let mut scene = r_graphics_engine::Scene::new(320, 240);
        session.with_active_in(|instance| unsafe {
            (*instance).current_renderplot_backend = Some(&mut scene);
            (*instance).portable_graphics.current =
                Some(crate::mainutils::portable_plot::Coordinates {
                    limits: [0., 1., 0., 1.],
                    rect: [50., 50., 270., 170.],
                    figure: [0., 0., 320., 240.],
                    device: [0., 0., 320., 240.],
                    log: [false; 2],
                });
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::draw_portable_positional(
                "mtext",
                input.as_raw(),
                &[
                    "text", "side", "line", "outer", "at", "adj", "padj", "cex", "col", "font",
                ],
            )
        }));
        session.with_active_in(|instance| unsafe { (*instance).current_renderplot_backend = None });
        assert_eq!(
            owner.sexp(result.unwrap()).unwrap().typeof_(),
            SEXPTYPE::NILSXP
        );
        assert!(fired.get());
        let text: Vec<_> = scene
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                r_graphics_engine::DrawOperation::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, ["first label", "second label"]);
    }

    #[test]
    fn positional_title_labels_survive_collecting_allocation() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        let mut input = factory.nil().into_owned().unwrap();
        for text in [
            "outer",
            "line",
            "y label",
            "x label",
            "subtitle",
            "main label",
            "C_title",
        ] {
            let value = if matches!(text, "outer" | "line") {
                factory.nil()
            } else {
                factory.strings(&[text]).unwrap()
            };
            input = factory
                .pairlist_cell(&value, &input, &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap();
        }
        // Input is the sole graph root for all label strings by this point.
        let fired = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = fired.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(true);
            crate::sexp::instance::with_required_current_instance(|instance| unsafe {
                (*instance).memory_state.gc_force_gap = 0;
            });
            crate::sexp::gengc::full_gc();
        }));
        let mut scene = r_graphics_engine::Scene::new(320, 240);
        session.with_active_in(|instance| unsafe {
            (*instance).current_renderplot_backend = Some(&mut scene);
            (*instance).portable_graphics.current =
                Some(crate::mainutils::portable_plot::Coordinates {
                    limits: [0., 1., 0., 1.],
                    rect: [50., 50., 270., 170.],
                    figure: [0., 0., 320., 240.],
                    device: [0., 0., 320., 240.],
                    log: [false; 2],
                });
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::draw_portable_title(input.as_raw())
        }));
        session.with_active_in(|instance| unsafe { (*instance).current_renderplot_backend = None });
        let value = owner.sexp(result.unwrap()).unwrap();
        assert_eq!(value.typeof_(), SEXPTYPE::NILSXP);
        assert!(fired.get());
        let text: Vec<_> = scene
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                r_graphics_engine::DrawOperation::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, ["main label", "x label", "y label", "subtitle"]);
    }
}

#[cfg(all(test, feature = "renderplot-device"))]
mod clip_and_convert_tests {
    use crate::sexp::session::RSession;

    /// Default 7-inch pdf geometry on a 504x504 scene: mar 5.1/4.1/4.1/2.1
    /// lines of 14.4 device units put the plot rectangle at
    /// [59.04, 59.04, 473.76, 430.56] with usr [0, 1] x [0, 1] under
    /// xaxs/yaxs = "i".
    const PLOT_RECT: [f32; 4] = [59.04, 59.04, 473.76, 430.56];
    const WINDOW: &str = "plot.new(); plot.window(xlim=c(0,1), ylim=c(0,1), xaxs='i', yaxs='i')";

    fn close(actual: &[f32; 4], expected: &[f32; 4]) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 0.01)
    }

    fn clip_rects(scene: &r_graphics_engine::Scene) -> Vec<[f32; 4]> {
        scene
            .operations()
            .iter()
            .filter_map(|operation| match operation {
                r_graphics_engine::DrawOperation::Clip(Some(rect)) => Some(*rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn clip_installs_the_manual_device_clip_for_subsequent_drawing() {
        let mut session = RSession::new();
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        let ok = session.eval_script_with_output_capture_then_renderplot(
            &format!("{WINDOW}; clip(0.25, 0.75, 0.25, 0.75); points(0.5, 0.5)"),
            &mut scene,
            |value, _, _| value.is_ok(),
        );
        assert!(ok, "clip probe should evaluate without error");
        // user (0.25, 0.25)..(0.75, 0.75) maps to the plot-rectangle quarter.
        let expected = [162.72, 151.92, 370.08, 337.68];
        let clips = clip_rects(&scene);
        assert!(
            clips.iter().any(|rect| close(rect, &expected)),
            "manual clip rect missing from {clips:?}"
        );
        // The point after clip() draws under the manual rectangle too.
        assert_eq!(clips.len(), 2);
        assert!(close(&clips[1], &expected));
    }

    #[test]
    fn clip_rectangle_is_clamped_to_the_device_region() {
        let mut session = RSession::new();
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        let ok = session.eval_script_with_output_capture_then_renderplot(
            &format!("{WINDOW}; clip(-10, 100, -10, 100)"),
            &mut scene,
            |value, _, _| value.is_ok(),
        );
        assert!(ok, "wide clip should evaluate without error");
        // GESetClip clamps each edge to the device region.
        let expected = [0., 0., 504., 504.];
        let clips = clip_rects(&scene);
        assert_eq!(clips.len(), 1);
        assert!(close(&clips[0], &expected), "clips: {clips:?}");
    }

    #[test]
    fn clip_does_not_survive_a_new_plot() {
        let mut session = RSession::new();
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        let ok = session.eval_script_with_output_capture_then_renderplot(
            &format!("{WINDOW}; clip(0.25, 0.75, 0.25, 0.75); plot.new(); points(0.5, 0.5)"),
            &mut scene,
            |value, _, _| value.is_ok(),
        );
        assert!(ok, "clip reset probe should evaluate without error");
        // plot.new starts a fresh frame (the scene is cleared) and GNewPlot's
        // GForceClip discards the manual rectangle: points() clips to the
        // plot region again.
        let clips = clip_rects(&scene);
        assert_eq!(clips.len(), 1);
        assert!(close(&clips[0], &PLOT_RECT), "clips: {clips:?}");
    }

    #[test]
    fn a_changed_xpd_region_restores_the_derived_clip() {
        let mut session = RSession::new();
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        let ok = session.eval_script_with_output_capture_then_renderplot(
            &format!(
                "{WINDOW}; clip(0.25, 0.75, 0.25, 0.75); segments(0.4, 0.4, 0.6, 0.6, xpd=TRUE)"
            ),
            &mut scene,
            |value, _, _| value.is_ok(),
        );
        assert!(ok, "xpd probe should evaluate without error");
        // GClip sees a region change and reverts to the figure clip.
        let clips = clip_rects(&scene);
        assert_eq!(clips.len(), 2);
        assert!(close(&clips[0], &[162.72, 151.92, 370.08, 337.68]));
        assert!(close(&clips[1], &[0., 0., 504., 504.]));
    }

    #[test]
    fn clip_rejects_nonfinite_coordinates_like_gnu() {
        let mut session = RSession::new();
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        let message = session.eval_script_with_output_capture_then_renderplot(
            &format!("{WINDOW}; clip(NaN, 2, 1, 2)"),
            &mut scene,
            |value, _, _| match value {
                Ok(_) => panic!("clip(NaN) should error"),
                Err(error) => error.to_string(),
            },
        );
        assert!(message.contains("invalid 'x1' argument"), "{message}");
    }

    #[test]
    fn grconvert_follows_usr_plt_and_ndc_math() {
        let mut session = RSession::new();
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        let values = session.eval_script_with_output_capture_then_renderplot(
            &format!(
                "{WINDOW}; c(grconvertX(0.5,'user','ndc'), \
                 grconvertX(0.5,'user','inches'), \
                 grconvertX(0.5,'user','npc'), \
                 grconvertY(0.5,'user','ndc'), \
                 grconvertY(0.5,'user','npc'), \
                 grconvertX(1,'inches','user'), \
                 grconvertX(1,'user','npc'))"
            ),
            &mut scene,
            |value, _, _| unsafe {
                // SAFETY: the session roots its result for this synchronous callback.
                let value = value.unwrap();
                (0..value.clone().len())
                    .map(|index| value.real_elt(index).unwrap())
                    .collect::<Vec<f64>>()
            },
        );
        let expected = [
            266.4 / 504.,
            266.4 / 72.,
            0.5,
            (504. - 244.8) / 504.,
            0.5,
            (72. - 59.04) / (473.76 - 59.04),
            1.,
        ];
        assert_eq!(values.len(), expected.len());
        // The plot rectangle is stored in f32, so converted locations carry
        // f32 rounding of the geometry.
        for (actual, expected) in values.iter().zip(&expected) {
            assert!(
                (actual - expected).abs() < 1e-6,
                "grconvert mismatch: {values:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn convert_rejects_invalid_unit_indices_like_gnu() {
        use crate::sexp::object::SessionNodeFactory;
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let factory = SessionNodeFactory::new(owner);
        // .External payload: routine name, values, from, to.
        let mut input = factory.nil().into_owned().unwrap();
        for index in (0..4).rev() {
            let value = match index {
                0 => factory.strings(&["C_convertX"]).unwrap(),
                1 => factory
                    .wrap(unsafe { crate::sexp::constructors::Rf_ScalarReal(0.5) })
                    .unwrap(),
                _ => factory
                    .wrap(unsafe { crate::sexp::constructors::Rf_ScalarInteger(0) })
                    .unwrap(),
            };
            input = factory
                .pairlist_cell(&value, &input, &factory.nil())
                .unwrap()
                .into_owned()
                .unwrap();
        }
        let mut scene = r_graphics_engine::Scene::new(504, 504);
        session.with_active_in(|instance| unsafe {
            (*instance).current_renderplot_backend = Some(&mut scene);
            (*instance).portable_graphics.current =
                Some(crate::mainutils::portable_plot::Coordinates {
                    limits: [0., 1., 0., 1.],
                    rect: [59.04, 59.04, 473.76, 430.56],
                    figure: [0., 0., 504., 504.],
                    device: [0., 0., 504., 504.],
                    log: [false; 2],
                });
        });
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::c_convert_x(input.as_raw())
        }))
        .expect_err("unit index 0 should error");
        session.with_active_in(|instance| unsafe { (*instance).current_renderplot_backend = None });
        let error = error
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("invalid unit index should use the R error path");
        assert!(error.message.contains("invalid 'from' argument"));
    }
}
