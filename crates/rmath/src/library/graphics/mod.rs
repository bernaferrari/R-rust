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
        std::ffi::CStr::from_ptr(chars)
            .to_string_lossy()
            .into_owned()
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
            let drawn = crate::mainutils::portable_plot::draw_builtin(name, built.as_raw());
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
                "title" => draw_portable_title(args),
                "polygon" => {
                    draw_portable_positional("polygon", args, &["x", "y", "col", "border", "lty"])
                }
                "box" | "segments" | "arrows" | "rect" | "abline" => forward_portable(bare, args),
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
            c_nil,
            crate::mainutils::native_routines::PayloadArity::Fixed(5),
        )),
        "convertX" | "convertY" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External1(
                c_nil,
                crate::mainutils::native_routines::PayloadArity::Fixed(3),
            ))
        }
        "persp" | "clip" | "dend" | "dendwindow" | "erase" | "path" | "raster" | "symbols"
        | "xspline" | "locator" | "identify" => {
            Some(crate::mainutils::native_routines::NativeRoutine::External1(
                c_nil,
                crate::mainutils::native_routines::PayloadArity::Variadic,
            ))
        }
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
