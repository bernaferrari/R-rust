//! PostScript stub and portable PDF device entry points.

use crate::sexp::accessors::*;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

/// Create a PostScript graphics device (postscript() function in R).
///
/// Opens the requested file and registers the device. Drawing commands are
/// not written; like [`PDF`] this mirrors GNU device bookkeeping so the
/// interpreter's device state tracks the open postscript() device.
pub fn PostScript(args: SEXP) -> SEXP {
    if !args.is_null() && args != R_NilValue() {
        let file = CADR(args);
        if !file.is_null() && TYPEOF(file) == SEXPTYPE::STRSXP && LENGTH(file) > 0 {
            let chars = STRING_ELT(file, 0);
            if chars != crate::sexp::globals::R_NaString() {
                if let Ok(path) = String::from_utf8(crate::sexp::accessors::charsxp_as_utf8(chars))
                {
                    if !path.is_empty() {
                        let _ = std::fs::write(path, b"%!PS-Adobe-3.0\n%%Pages: 1\n");
                    }
                }
            }
        }
    }
    crate::library::grdevices::device_registry::selectDevice(0);
    let base = crate::sexp::globals::R_BaseEnv();
    let sym = crate::sexp::symbol::install_str(".Devices");
    let old = crate::sexp::envir::R_findVarInFrame(base, sym);
    let n = if !old.is_null()
        && old != crate::sexp::globals::R_UnboundValue()
        && TYPEOF(old) == SEXPTYPE::VECSXP
    {
        XLENGTH(old)
    } else {
        0
    };
    let devices = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, (n + 1) as i32);
    for i in 0..n {
        SET_VECTOR_ELT(devices, i, VECTOR_ELT(old, i));
    }
    if n == 0 {
        SET_VECTOR_ELT(
            devices,
            0,
            crate::sexp::constructors::mk_string_str("null device"),
        );
    }
    SET_VECTOR_ELT(
        devices,
        if n == 0 { 0i64 } else { n },
        crate::sexp::constructors::mk_string_str("postscript"),
    );
    // The n==0 arm wrote null device at 0 and then overwrites it. Fix below.
    if n == 0 {
        let both = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::VECSXP, 2);
        SET_VECTOR_ELT(
            both,
            0,
            crate::sexp::constructors::mk_string_str("null device"),
        );
        SET_VECTOR_ELT(
            both,
            1,
            crate::sexp::constructors::mk_string_str("postscript"),
        );
        crate::sexp::envir::defineVar(sym, both, base);
    } else {
        crate::sexp::envir::defineVar(sym, devices, base);
    }
    // GNU initDevice() sets .Device to the newly opened device's name.
    crate::sexp::envir::defineVar(
        crate::sexp::symbol::install_str(".Device"),
        crate::sexp::constructors::mk_string_str("postscript"),
        base,
    );
    R_NilValue()
}

/// Open the ordinary grDevices PDF with a device-owned recording.
pub fn PDF(args: SEXP) -> SEXP {
    use crate::sexp::context::r_error;
    let mut args = if args.is_null() {
        R_NilValue()
    } else {
        CDR(args)
    };
    let mut next = || {
        if args == R_NilValue() {
            return R_NilValue();
        }
        let value = CAR(args);
        args = CDR(args);
        value
    };
    let file = next();

    #[cfg(not(feature = "renderplot-device"))]
    {
        if file != R_NilValue() {
            r_error("file-backed PDF graphics require the portable graphics device");
        }
        super::device_registry::open_null_pdf();
    }
    #[cfg(feature = "renderplot-device")]
    {
        use crate::library::graphics::par::{ParValue, set_plot_parameter};
        use crate::mainutils::coerce::{asLogical, asReal};
        let paper = pdf_string(next(), "special");
        let family = pdf_string(next(), "Helvetica");
        let _encoding = next();
        let bg = pdf_string(next(), "transparent");
        let fg = pdf_string(next(), "black");
        let number = |value, default| {
            if value == R_NilValue() {
                default
            } else {
                asReal(value)
            }
        };
        let width = number(next(), 7.0);
        let height = number(next(), 7.0);
        let points = number(next(), 12.0);
        let onefile = next();
        if file != R_NilValue() && onefile != R_NilValue() && asLogical(onefile) != 1 {
            r_error("onefile = FALSE is not supported by this PDF device");
        }
        let _pagecentre = next();
        let _title = next();
        let fonts = next();
        let version_major = number(next(), 1.0);
        let version_minor = number(next(), 4.0);
        let color_model = pdf_string(next(), "srgb");
        let _dingbats = next();
        let _kerning = next();
        let _odd_even = next();
        let compress = next();
        let compress = compress == R_NilValue() || asLogical(compress) == 1;
        if paper != "special" || !matches!(color_model.as_str(), "srgb" | "rgb") {
            r_error("the PDF device requires paper = 'special' and an RGB color model");
        }
        let dimension = |inches: f64| {
            let points = (inches * 72.0).round();
            if !(1.0..=f64::from(u16::MAX)).contains(&points) {
                r_error("invalid PDF device dimensions");
            }
            points as u32
        };
        if file != R_NilValue()
            && (!matches!(family.as_str(), "Helvetica" | "sans")
                || (fonts != R_NilValue() && LENGTH(fonts) > 0)
                || (version_major, version_minor) != (1.0, 4.0))
        {
            r_error("the PDF device supports the bundled sans faces and PDF version 1.4");
        }
        let dimensions = (dimension(width), dimension(height));
        if !points.is_finite() || points < 1.0 || points > f64::from(i32::MAX) {
            r_error("invalid PDF point size");
        }
        let output = if file == R_NilValue() {
            None
        } else {
            let path = pdf_string(file, "");
            if path.is_empty() || path.contains('%') || path.starts_with('|') {
                r_error("the PDF device requires a literal file name");
            }
            Some(
                super::pdf_output::PdfOutput::new(&path)
                    .unwrap_or_else(|error| r_error(format!("cannot open PDF '{path}': {error}"))),
            )
        };
        super::device_registry::open_pdf(output, width, height, dimensions, compress);
        set_plot_parameter("bg", ParValue::String(bg));
        set_plot_parameter("col", ParValue::String(fg.clone()));
        set_plot_parameter("fg", ParValue::String(fg));
        set_plot_parameter("ps", ParValue::Integer(vec![points.round() as i32]));
    }
    R_NilValue()
}

#[cfg(feature = "renderplot-device")]
fn pdf_string(value: SEXP, default: &str) -> String {
    {
        if value == R_NilValue() {
            return default.to_owned();
        }
        if TYPEOF(value) != SEXPTYPE::STRSXP
            || LENGTH(value) != 1
            || STRING_ELT(value, 0) == crate::sexp::globals::R_NaString()
        {
            crate::sexp::context::r_error("invalid PDF device argument");
        }
        String::from_utf8(crate::sexp::accessors::charsxp_as_utf8(STRING_ELT(
            value, 0,
        )))
        .unwrap_or_else(|_| crate::sexp::context::r_error("invalid PDF argument encoding"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn postscript_device_writes_ps_stub_and_registers() {
        let _session = RSession::new();
        let path = std::env::temp_dir().join(format!("rport-ps-stub-{}.ps", std::process::id()));
        let path_string = path.to_string_lossy().into_owned();

        unsafe {
            use crate::sexp::accessors::{CHAR, STRING_ELT, TYPEOF};
            use crate::sexp::constructors::{Rf_cons, Rf_mkString};
            use crate::sexp::ffi::SEXPTYPE;

            // CADR(args) carries the file argument of the external call.
            let file = crate::sexp::constructors::mk_string_bytes(
                std::ffi::CString::new(path_string.as_str())
                    .expect("portable temp path")
                    .to_bytes(),
            );
            let args = Rf_cons(R_NilValue(), Rf_cons(file, R_NilValue()));
            let result = PostScript(args);
            assert!(result.is_null() || result == R_NilValue());

            // The device writes its PostScript stub header to the file.
            assert_eq!(
                std::fs::read(&path).expect("postscript stub file"),
                b"%!PS-Adobe-3.0\n%%Pages: 1\n"
            );

            // initDevice() bookkeeping: .Device names the opened device.
            let device = crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                crate::sexp::symbol::install_str(".Device"),
            );
            assert_eq!(TYPEOF(device), SEXPTYPE::STRSXP);
            assert_eq!(
                &crate::sexp::accessors::char_cstr_bytes(STRING_ELT(device, 0))[..],
                b"postscript"
            );

            // Selecting the null device opened a real registry device.
            assert_eq!(crate::library::grdevices::device_registry::NoDevices(), 0);
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pdf_device_opens_without_error() {
        let _session = RSession::new();
        let result = PDF(R_NilValue());
        assert!(result.is_null() || result == R_NilValue());
    }
}
