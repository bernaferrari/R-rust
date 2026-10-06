//! grDevices package - graphics devices

pub(crate) mod axis_scales;
mod chull;
mod clippath;
pub(crate) mod colors;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod devcairo;
pub(crate) mod device_registry;
mod devices;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod devpictex;
pub(crate) mod devps;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod devquartz;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub(crate) mod devwindows;
mod group;
mod init;
mod mask;
mod patterns;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod qdbitmap;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod qdpdf;
mod stubs;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod winbitmap;

unsafe extern "C-unwind" fn c_pdf(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devps::PDF(args) }
}
unsafe extern "C-unwind" fn c_cairo_props(which: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { init::cairoProps(which) }
}
unsafe extern "C-unwind" fn c_dev_ask_new_page(
    call: crate::sexp::ffi::SEXP,
    op: crate::sexp::ffi::SEXP,
    args: crate::sexp::ffi::SEXP,
    env: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { stubs::devAskNewPage(call, op, args, env) }
}
unsafe extern "C-unwind" fn c_devholdflush(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devholdflush(args) }
}
unsafe extern "C-unwind" fn c_devcur(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devcur(args) }
}
unsafe extern "C-unwind" fn c_devoff(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devoff(args) }
}
unsafe extern "C-unwind" fn c_devset(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devset(args) }
}
unsafe extern "C-unwind" fn c_devcontrol(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devcontrol(args) }
}
unsafe extern "C-unwind" fn c_devdisplaylist(
    args: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devdisplaylist(args) }
}
unsafe extern "C-unwind" fn c_devcap(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devcap(args) }
}
unsafe extern "C-unwind" fn c_devsize(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devsize(args) }
}
unsafe extern "C-unwind" fn c_devnext(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devnext(args) }
}
unsafe extern "C-unwind" fn c_devprev(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { devices::devprev(args) }
}
unsafe extern "C-unwind" fn c_palette2(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
    unsafe { colors::do_palette2(args) }
}
unsafe extern "C-unwind" fn c_col2rgb(
    colors: crate::sexp::ffi::SEXP,
    alpha: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { self::colors::do_col2rgb(colors, alpha) }
}
unsafe extern "C-unwind" fn c_create_at(
    axp: crate::sexp::ffi::SEXP,
    usr: crate::sexp::ffi::SEXP,
    nint: crate::sexp::ffi::SEXP,
    is_log: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { axis_scales::R_CreateAtVector(axp, usr, nint, is_log) }
}
unsafe extern "C-unwind" fn c_g_axis_pars(
    usr: crate::sexp::ffi::SEXP,
    is_log: crate::sexp::ffi::SEXP,
    nint: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { axis_scales::R_GAxisPars(usr, is_log, nint) }
}
unsafe extern "C-unwind" fn c_gray(
    lev: crate::sexp::ffi::SEXP,
    alpha: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { colors::do_gray(lev, alpha) }
}
unsafe extern "C-unwind" fn c_hsv(
    h: crate::sexp::ffi::SEXP,
    s: crate::sexp::ffi::SEXP,
    v: crate::sexp::ffi::SEXP,
    a: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { colors::do_hsv(h, s, v, a) }
}
unsafe extern "C-unwind" fn c_hcl(
    h: crate::sexp::ffi::SEXP,
    c: crate::sexp::ffi::SEXP,
    l: crate::sexp::ffi::SEXP,
    a: crate::sexp::ffi::SEXP,
    fixup: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { colors::do_hcl(h, c, l, a, fixup) }
}
unsafe extern "C-unwind" fn c_rgb(
    r: crate::sexp::ffi::SEXP,
    g: crate::sexp::ffi::SEXP,
    b: crate::sexp::ffi::SEXP,
    a: crate::sexp::ffi::SEXP,
    mcv: crate::sexp::ffi::SEXP,
    nam: crate::sexp::ffi::SEXP,
) -> crate::sexp::ffi::SEXP {
    unsafe { colors::do_rgb(r, g, b, a, mcv, nam) }
}

pub(crate) fn lookup(name: &str) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "cairoProps" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_cairo_props),
        )),
        "devAskNewPage" => Some(crate::mainutils::native_routines::NativeRoutine::External2(
            c_dev_ask_new_page,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "PDF" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_pdf,
            crate::mainutils::native_routines::PayloadArity::Fixed(23),
        )),
        "palette2" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args1(c_palette2),
        )),
        "devholdflush" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devholdflush,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "devcur" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devcur,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        "devoff" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devoff,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "devset" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devset,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "devcontrol" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devcontrol,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "devdisplaylist" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devdisplaylist,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        "devcap" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devcap,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "devsize" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devsize,
            crate::mainutils::native_routines::PayloadArity::Fixed(0),
        )),
        "devnext" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devnext,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "devprev" => Some(crate::mainutils::native_routines::NativeRoutine::External1(
            c_devprev,
            crate::mainutils::native_routines::PayloadArity::Fixed(1),
        )),
        "R_CreateAtVector" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_create_at),
        )),
        "R_GAxisPars" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args3(c_g_axis_pars),
        )),
        "gray" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_gray),
        )),
        "col2rgb" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args2(c_col2rgb),
        )),
        "hsv" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args4(c_hsv),
        )),
        "hcl" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args5(c_hcl),
        )),
        "rgb" => Some(crate::mainutils::native_routines::NativeRoutine::Call(
            crate::mainutils::native_routines::CallRoutine::Args6(c_rgb),
        )),
        _ => None,
    }
}

/// One-argument `.External` device routines. Not `R_CreateAtVector` or `R_GAxisPars`.
pub(crate) fn lookup_external(
    name: &str,
) -> Option<crate::mainutils::native_routines::NativeRoutine> {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "PDF" | "devholdflush" | "devcur" | "devoff" | "devset" | "devcontrol"
        | "devdisplaylist" | "devcap" | "devsize" | "devnext" | "devprev" => lookup(name),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: crate::sexp::ffi::SEXP) {
    unsafe {
        for name in [
            "C_cairoProps",
            "C_PDF",
            "C_palette2",
            "C_devholdflush",
            "C_devAskNewPage",
            "C_devcur",
            "C_devoff",
            "C_devset",
            "C_devcontrol",
            "C_devdisplaylist",
            "C_devcap",
            "C_devsize",
            "C_devnext",
            "C_devprev",
            "C_R_CreateAtVector",
            "C_R_GAxisPars",
            "C_gray",
            "C_col2rgb",
            "C_hsv",
            "C_hcl",
            "C_rgb",
            "C_getSnapshot",
            "C_playSnapshot",
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
