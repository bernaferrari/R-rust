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

pub fn lookup(name: &str) -> crate::unix::dynload::DL_FUNC {
    match name {
        "PDF" | "C_PDF" => Some(unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(crate::sexp::ffi::SEXP) -> crate::sexp::ffi::SEXP,
                _,
            >(devps::PDF)
        }),
        _ => None,
    }
}

pub unsafe fn install_call_symbols(env: crate::sexp::ffi::SEXP) {
    unsafe {
        let cname = std::ffi::CString::new("C_PDF").unwrap_or_default();
        crate::sexp::envir::defineVar(
            crate::sexp::symbol::Rf_install(cname.as_ptr()),
            crate::sexp::constructors::Rf_mkString(cname.as_ptr()),
            env,
        );
    }
}
