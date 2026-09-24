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
unsafe extern "C-unwind" fn c_devdisplaylist(args: crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP {
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

fn as_ext(f: unsafe extern "C-unwind" fn(crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP) -> crate::unix::dynload::DL_FUNC {
    Some(unsafe { std::mem::transmute(f) })
}

pub fn lookup(name: &str) -> crate::unix::dynload::DL_FUNC {
    let bare = name.strip_prefix("C_").unwrap_or(name);
    match bare {
        "PDF" => as_ext(c_pdf),
        "palette2" => as_ext(c_palette2),
        "devholdflush" => as_ext(c_devholdflush),
        "devcur" => as_ext(c_devcur),
        "devoff" => as_ext(c_devoff),
        "devset" => as_ext(c_devset),
        "devcontrol" => as_ext(c_devcontrol),
        "devdisplaylist" => as_ext(c_devdisplaylist),
        "devcap" => as_ext(c_devcap),
        "devsize" => as_ext(c_devsize),
        "devnext" => as_ext(c_devnext),
        "devprev" => as_ext(c_devprev),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: crate::sexp::ffi::SEXP) {
    unsafe {
        for name in [
            "C_PDF",
            "C_palette2",
            "C_devholdflush",
            "C_devcur",
            "C_devoff",
            "C_devset",
            "C_devcontrol",
            "C_devdisplaylist",
            "C_devcap",
            "C_devsize",
            "C_devnext",
            "C_devprev",
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
