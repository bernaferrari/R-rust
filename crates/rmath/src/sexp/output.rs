//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use super::accessors::{
    ATTRIB, CAR, CDR, CHAR, PRINTNAME, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
use super::ffi::{NA_INTEGER, R_IsNA, R_IsNaN, R_xlen_t, SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::instance::RInstance;
use super::object::Sexp;

/// Captured R output.
#[derive(Debug, Clone, Default)]
pub struct RCapturedOutput {
    pub stdout: String,
    pub stderr: String,
    /// Chronological stdout+stderr for embed hosts (GNU terminal order).
    pub interleaved: String,
    pub truncated: bool,
}


/// One capture layer owns its budget, including traffic forwarded to it.
#[derive(Debug, Default)]
struct CaptureFrame {
    stdout: Option<String>,
    stderr: Option<String>,
    interleaved: Option<String>,
    truncated: bool,
    used_bytes: usize,
    split_stdout: bool,
    sink_depth_at_start: usize,
    connection: Option<(*mut RInstance, i32)>,
}


#[derive(Clone, Copy)]
enum OutputStream {
    Stdout,
    Stderr,
    Message,
}

impl CaptureFrame {
    /// Return true when this layer consumes the stream completely.
    fn write(&mut self, stream: OutputStream, msg: &str, limit: Option<usize>) -> bool {
        let (target, split) = match stream {
            OutputStream::Stdout => (&mut self.stdout, self.split_stdout),
            OutputStream::Stderr => (&mut self.stderr, false),
            // GNU message() writes stderr. An explicit message capture
            // (capture.output(type="message")) owns stderr; an output-only
            // capture must let messages continue outward as stderr.
            OutputStream::Message if self.stderr.is_some() => (&mut self.stderr, false),
            OutputStream::Message => return false,
        };
        if let Some(buffer) = target {
            if let Some((owner, connection)) = self.connection {
                if super::instance::current_instance_ptr() != Some(owner) {
                    std::panic::panic_any(super::context::RError {
                        message: "capture destination belongs to another session".into(),
                    });
                }
                crate::mainutils::connections::connection_write_bytes(connection, msg.as_bytes());
                return !split;
            }
            append_bounded(
                buffer,
                msg,
                limit,
                &mut self.used_bytes,
                &mut self.truncated,
            );
            if let Some(interleaved) = &mut self.interleaved {
                interleaved.push_str(msg);
            }
            return !split;

        }
        false
    }
    fn active(&self) -> bool {
        self.stdout.is_some() || self.stderr.is_some()
    }
}

/// Per-session output capture buffers.
#[derive(Debug, Default)]
pub(crate) struct OutputCaptureState {
    current: CaptureFrame,
    stack: Vec<CaptureFrame>,
    max_bytes: Option<usize>,
}

impl OutputCaptureState {
    pub(crate) fn start(&mut self) {
        self.start_with_options(true, true, false);
    }

    pub(crate) fn start_with_options(&mut self, stdout: bool, stderr: bool, split: bool) {
        let frame = CaptureFrame {
            stdout: stdout.then(String::new),
            stderr: stderr.then(String::new),
            interleaved: (stdout || stderr).then(String::new),
            split_stdout: split,
            ..CaptureFrame::default()
        };

        let outer = std::mem::replace(&mut self.current, frame);
        if outer.active() {
            self.stack.push(outer);
        }
    }

    pub(crate) fn uses_connection(&self, index: i32) -> bool {
        self.current
            .connection
            .is_some_and(|(_, connection)| connection == index)
            || self.stack.iter().any(|frame| {
                frame
                    .connection
                    .is_some_and(|(_, connection)| connection == index)
            })
    }

    pub(crate) fn stop(&mut self) -> RCapturedOutput {
        let frame = std::mem::replace(&mut self.current, self.stack.pop().unwrap_or_default());
        RCapturedOutput {
            stdout: frame.stdout.unwrap_or_default(),
            stderr: frame.stderr.unwrap_or_default(),
            interleaved: frame.interleaved.unwrap_or_default(),
            truncated: frame.truncated,
        }

    }

    pub(crate) fn is_capturing(&self) -> bool {
        self.current.active()
    }

    fn route(&mut self, stream: OutputStream, msg: &str) -> bool {
        self.route_with_sink(stream, msg, None)
    }
    fn route_with_sink(
        &mut self,
        stream: OutputStream,
        msg: &str,
        instance: Option<*mut RInstance>,
    ) -> bool {
        let mut pending_depth = instance.map_or(0, |instance| unsafe {
            (*instance).connections_state.sink.sink_number
        });
        for frame in std::iter::once(&mut self.current).chain(self.stack.iter_mut().rev()) {
            if matches!(stream, OutputStream::Stdout) && frame.stdout.is_some() {
                let floor = if frame.stderr.is_some() {
                    0
                } else {
                    frame.sink_depth_at_start.min(pending_depth)
                };
                if let Some(instance) = instance {
                    if crate::mainutils::connections::write_output_sinks_between(
                        instance,
                        msg.as_bytes(),
                        floor,
                        pending_depth,
                    ) {
                        return true;
                    }
                }
                pending_depth = floor;
            }
            if frame.write(stream, msg, self.max_bytes) {
                return true;
            }
        }
        false
    }
    pub(crate) fn capture_stdout(&mut self, msg: &str) -> bool {
        self.route(OutputStream::Stdout, msg)
    }
    pub(crate) fn capture_stderr(&mut self, msg: &str) -> bool {
        self.route(OutputStream::Stderr, msg)
    }
    pub(crate) fn set_max_bytes(&mut self, max_bytes: Option<usize>) {
        self.max_bytes = max_bytes;
    }
}

fn append_bounded(
    target: &mut String,
    message: &str,
    max_bytes: Option<usize>,
    used_bytes: &mut usize,
    truncated: &mut bool,
) {
    let Some(limit) = max_bytes else {
        target.push_str(message);
        return;
    };
    if message.is_empty() {
        return;
    }
    if *used_bytes >= limit {
        *truncated = true;
        return;
    }
    let remaining = limit - *used_bytes;
    if message.len() <= remaining {
        target.push_str(message);
        *used_bytes += message.len();
        return;
    }
    let mut end = remaining;
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&message[..end]);
    *used_bytes += end;
    *truncated = true;
}

thread_local! {
    static PRINT_DISPATCH_EXTRAS: std::cell::Cell<SEXP> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

pub(crate) struct PrintDispatchExtrasGuard {
    previous: SEXP,
}

impl Drop for PrintDispatchExtrasGuard {
    fn drop(&mut self) {
        PRINT_DISPATCH_EXTRAS.with(|slot| slot.set(self.previous));
    }
}

/// Forward extra `print(...)` arguments to recursive `print.<class>` methods.
pub(crate) fn push_print_dispatch_extras(extras: SEXP) -> PrintDispatchExtrasGuard {
    let previous = PRINT_DISPATCH_EXTRAS.with(|slot| slot.replace(extras));
    PrintDispatchExtrasGuard { previous }
}

pub(crate) fn print_dispatch_extras() -> SEXP {
    PRINT_DISPATCH_EXTRAS.with(|slot| slot.get())
}

pub(crate) unsafe fn cons_print_args(x: SEXP) -> SEXP {
    unsafe {
        let extras = print_dispatch_extras();
        let extras = if extras.is_null() { R_NilValue() } else { extras };
        crate::sexp::constructors::Rf_cons(x, extras)
    }
}


unsafe fn print_args_except_x(args: SEXP, x: SEXP) -> SEXP {
    unsafe {
        let mut head = R_NilValue();
        let mut tail = R_NilValue();
        let mut cur = args;
        let mut skipped_x = false;
        while !cur.is_null() && cur != R_NilValue() {
            let value = CAR(cur);
            if !skipped_x && value == x {
                skipped_x = true;
            } else {
                let cell = crate::sexp::constructors::Rf_cons(value, R_NilValue());
                crate::sexp::accessors::SETTAG(cell, TAG(cur));
                if head == R_NilValue() {
                    head = cell;
                } else {
                    crate::sexp::accessors::SETCDR(tail, cell);
                }
                tail = cell;
            }
            cur = CDR(cur);
        }
        head
    }
}

/// Copy every `print()` argument except `x` for recursive method dispatch.
pub(crate) unsafe fn copy_print_dispatch_extras(args: SEXP, x: SEXP) -> SEXP {
    unsafe { print_args_except_x(args, x) }
}


/// Start capturing R output.
pub fn start_capture() {
    super::instance::with_required_current_instance(start_capture_in);
}

pub(crate) fn start_capture_in(inst: *mut RInstance) {
    // P2: strictly-local RefCell write; no ambient write intervenes.
    unsafe {
        (*inst).output_capture.borrow_mut().start();
    }
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    super::instance::with_required_current_instance(stop_capture_in)
}

pub(crate) fn stop_capture_in(inst: *mut RInstance) -> RCapturedOutput {
    // P2: strictly-local RefCell read/write; no ambient write intervenes.
    unsafe { (*inst).output_capture.borrow_mut().stop() }
}

/// A nested capture owned by its starting session, restored even on errors.
/// Discarding an unfinished capture mirrors capture.output(file=NULL) on error.
pub(crate) struct OutputCaptureGuard {
    instance: *mut RInstance,
    active: bool,
}
impl OutputCaptureGuard {
    pub(crate) fn start() -> Self {
        let instance = super::instance::with_required_current_instance(|instance| instance);
        start_capture_in(instance);
        Self {
            instance,
            active: true,
        }
    }
    pub(crate) fn start_with_options(stdout: bool, stderr: bool, split: bool) -> Self {
        let instance = super::instance::with_required_current_instance(|instance| instance);
        unsafe {
            let depth = (*instance).connections_state.sink.sink_number;
            let mut capture = (*instance).output_capture.borrow_mut();
            capture.start_with_options(stdout, stderr, split);
            capture.current.sink_depth_at_start = depth;
        }
        Self {
            instance,
            active: true,
        }
    }
    pub(crate) fn set_connection(&mut self, index: i32) {
        unsafe {
            (*self.instance)
                .output_capture
                .borrow_mut()
                .current
                .connection = Some((self.instance, index));
        }
    }
    pub(crate) fn finish(mut self) -> RCapturedOutput {
        let output = stop_capture_in(self.instance);
        self.active = false;
        output
    }
}
impl Drop for OutputCaptureGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = stop_capture_in(self.instance);
        }
    }
}

/// Check if output capture is active.
pub fn is_capturing() -> bool {
    super::instance::with_current_instance(is_capturing_in).unwrap_or(false)
}

pub(crate) fn is_capturing_in(inst: *mut RInstance) -> bool {
    // P2: strictly-local RefCell read; no ambient write intervenes.
    let capture_active = unsafe { (*inst).output_capture.borrow().is_capturing() };
    capture_active || crate::mainutils::connections::output_sink_active_in(inst)
}

/// Append to captured stdout. Called by the Rprintf hook.
pub fn capture_stdout(msg: &str) {
    super::instance::with_current_instance(|inst| capture_stdout_in(inst, msg));
}

pub(crate) fn capture_stdout_in(inst: *mut RInstance, msg: &str) {
    // P2: the RefCell borrow below is dropped before the print!, and no
    // ambient write occurs while it is held.
    let mut capture = unsafe { (*inst).output_capture.borrow_mut() };
    if capture.route_with_sink(OutputStream::Stdout, msg, Some(inst)) {
        return;
    }
    drop(capture);
    if !crate::mainutils::connections::write_output_sink_in(inst, msg.as_bytes()) {
        print!("{msg}");
    }
}

/// Append to the session's single interleaved output stream — the stdout
/// capture buffer, bypassing any `sink()` diversion — falling back to real
/// stderr when no capture is active. Signal-time message() emission uses
/// this: upstream writes messages to stderr and the terminal interleaves
/// the two streams in real time; the session model keeps one ordered stream
/// so the text lands in statement order between print() side effects,
/// deferred warnings, and auto-printed values.
pub(crate) fn capture_interleaved(msg: &str) {
    super::instance::with_current_instance(|inst| unsafe {
        let captured = (*inst)
            .output_capture
            .borrow_mut()
            .route(OutputStream::Message, msg);
        if !captured {
            eprint!("{msg}");
        }
    });
}
/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    super::instance::with_current_instance(|inst| capture_stderr_in(inst, msg));
}

pub(crate) fn capture_stderr_in(inst: *mut RInstance, msg: &str) {
    // P2: strictly-local RefCell write; no ambient write intervenes.
    unsafe {
        let mut capture = (*inst).output_capture.borrow_mut();
        if !capture.capture_stderr(msg) {
            drop(capture);
            eprint!("{msg}");
        }
    }
}

pub(crate) fn format_sexp(x: SEXP) -> String {
    if x.is_null() {
        return "NULL".to_string();
    }
    if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
        format_sexp_direct(sexp)
    } else {
        "NULL".to_string()
    }
}

fn format_aligned_values(vals: Vec<String>) -> String {
    let width = vals.iter().map(|v| v.len()).max().unwrap_or(0);
    vals.into_iter()
        .map(|v| format!("{v:>width$}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_named_values(names: &[String], values: &[String]) -> String {
    let widths: Vec<usize> = names
        .iter()
        .zip(values)
        .map(|(name, value)| {
            let width = name.len().max(value.len());
            if name == "<NA>" { width.max(5) } else { width }
        })
        .collect();
    let name_line = names
        .iter()
        .zip(&widths)
        .map(|(name, width)| format!("{name:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    let value_line = values
        .iter()
        .zip(&widths)
        .map(|(value, width)| format!("{value:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{name_line}\n{value_line}")
}

fn format_integer_value(v: i32) -> String {
    if v == NA_INTEGER {
        "NA".to_string()
    } else {
        v.to_string()
    }
}

pub(crate) fn format_real_value(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    if R_IsNA(v) {
        "NA".to_string()
    } else if R_IsNaN(v) {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v.is_sign_negative() {
            "-Inf".to_string()
        } else {
            "Inf".to_string()
        }
    } else if v.fract() == 0.0 && !needs_scientific(v) {
        format!("{v:.0}")
    } else {
        format_r_default_real(v)
    }
}

fn needs_scientific(v: f64) -> bool {
    if !v.is_finite() || v == 0.0 {
        return false;
    }
    let digits = unsafe { crate::mainutils::format::format_get_R_print().digits }.max(1);
    let exponent = v.abs().log10().floor() as i32;
    !(-4..digits).contains(&exponent)
}

fn is_finite_r_number(v: f64) -> bool {
    !R_IsNA(v) && !R_IsNaN(v) && v.is_finite()
}

fn format_real_value_for_vector(v: f64, force_decimal_for_whole: bool) -> String {
    if force_decimal_for_whole && is_finite_r_number(v) && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format_real_value(v)
    }
}

fn format_real_vector_values(x: Sexp<'_>, limit: R_xlen_t) -> Vec<String> {
    let values: Vec<_> = (0..x.clone().len().min(limit))
        .map(|i| x.clone().try_real_elt(i))
        .collect();
    let force_decimal_for_whole = values
        .iter()
        .filter_map(|value| value.as_ref().ok().copied())
        .any(|value| is_finite_r_number(value) && value.fract() != 0.0);

    values
        .into_iter()
        .map(|value| {
            value
                .map(|value| format_real_value_for_vector(value, force_decimal_for_whole))
                .unwrap_or_else(format_access_error)
        })
        .collect()
}

fn trim_float(s: String) -> String {
    let (mut mantissa, exponent) = match s.find(['e', 'E']) {
        Some(idx) => (s[..idx].to_string(), s[idx..].to_string()),
        None => (s, String::new()),
    };
    if mantissa.contains('.') {
        while mantissa.ends_with('0') {
            mantissa.pop();
        }
        if mantissa.ends_with('.') {
            mantissa.pop();
        }
    }
    let exponent = if exponent.len() >= 2 {
        let mark = &exponent[..1];
        let rest = &exponent[1..];
        if rest.starts_with('+') || rest.starts_with('-') {
            format!("{mark}{rest}")
        } else {
            format!("{mark}+{rest}")
        }
    } else {
        exponent
    };
    format!("{mantissa}{exponent}")
}



fn format_r_default_real(v: f64) -> String {
    let digits = unsafe { crate::mainutils::format::format_get_R_print().digits }.max(1);
    let abs = v.abs();
    if abs == 0.0 {
        return "0".to_string();
    }

    let exponent = abs.log10().floor() as i32;
    if !(-4..digits).contains(&exponent) {
        let decimals = (digits as usize).saturating_sub(1);
        return trim_float(format!("{v:.decimals$e}"));
    }


    let decimals = if exponent >= 0 {
        (digits - exponent - 1).max(0) as usize
    } else {
        (digits - exponent - 1) as usize
    };
    trim_float(format!("{v:.decimals$}"))
}

fn format_logical_value(v: i32) -> String {
    match v {
        0 => "FALSE".to_string(),
        1 => "TRUE".to_string(),
        _ => "NA".to_string(),
    }
}

pub(crate) fn format_complex_value(v: super::ffi::Rcomplex) -> String {
    if R_IsNA(v.r) || R_IsNA(v.i) {
        return "NA".to_string();
    }
    // EncodeReal0: x == 0.0 becomes +0, so cat/print of -0i is "+0i".
    let imag = if v.i == 0.0 { 0.0 } else { v.i };
    let real = format_real_value(v.r);
    let imaginary = format_real_value(imag.abs());
    if imag.is_sign_negative() {
        format!("{real}-{imaginary}i")
    } else {
        format!("{real}+{imaginary}i")
    }
}


fn format_access_error(err: impl std::fmt::Display) -> String {
    format!("<{err}>")
}

fn format_integer_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_integer_elt(i)
        .map(format_integer_value)
        .unwrap_or_else(format_access_error)
}

fn format_real_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_real_elt(i)
        .map(format_real_value)
        .unwrap_or_else(format_access_error)
}

fn format_logical_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_logical_elt(i)
        .map(format_logical_value)
        .unwrap_or_else(format_access_error)
}

fn format_complex_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_complex_elt(i)
        .map(format_complex_value)
        .unwrap_or_else(format_access_error)
}

pub(crate) fn format_raw_value(v: u8) -> String {

    format!("{v:02x}")
}

fn printable_attribute_name(attr: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(attr);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let print_name = PRINTNAME(tag);
        if print_name.is_null() || print_name == R_NilValue() {
            return None;
        }
        let chars = CHAR(print_name);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

fn is_structural_print_attribute(name: &str) -> bool {
    matches!(name, "names" | "dim" | "dimnames" | "row.names")
}

fn is_hidden_noquote_class(name: &str, x: Sexp<'_>) -> bool {
    name == "class" && has_class(x, "noquote")
}



fn format_printable_attributes(x: Sexp<'_>) -> String {
    unsafe {
        let mut attrs = ATTRIB(x.clone().as_raw());

        let mut visible = Vec::new();
        while !attrs.is_null() && attrs != R_NilValue() {
            if let Some(name) = printable_attribute_name(attrs)
                && !is_structural_print_attribute(&name)
                && !is_hidden_noquote_class(&name, x.clone())
            {
                let value = CAR(attrs);
                if !value.is_null() && value != R_NilValue() {
                    visible.push((name, value));
                }
            }
            attrs = CDR(attrs);
        }

        let mut out = String::new();
        for (name, value) in visible {
            out.push('\n');
            out.push_str(&format!("attr(,\"{name}\")\n"));
            if let Some(value) = Sexp::from_raw(value) {
                if let Some(dispatched) = format_dispatched_print(value.clone()) {
                    out.push_str(&dispatched);
                } else {
                    out.push_str(&format_sexp_direct(value));
                }
            } else {
                out.push_str("NULL");
            }
        }
        out
    }
}


fn format_list_body_with_attributes(body: String, x: Sexp<'_>) -> String {
    let attrs = format_printable_attributes(x);
    if attrs.is_empty() {
        format!("{body}\n")
    } else {
        format!("{body}\n{attrs}")

    }
}






fn format_with_printable_attributes(base: String, x: Sexp<'_>) -> String {
    format!("{base}{}", format_printable_attributes(x))
}

fn matrix_dims(x: Sexp<'_>) -> Option<(usize, usize)> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim = Sexp::from_raw(dim)?;
        if dim.clone().typeof_() != SEXPTYPE::INTSXP || dim.clone().len() != 2 {
            return None;
        }
        let nrow = dim.clone().integer_elt(0)? as usize;
        let ncol = dim.integer_elt(1)? as usize;
        if nrow.checked_mul(ncol)? > x.len() as usize {
            return None;
        }
        Some((nrow, ncol))
    }
}

struct MatrixDimnames {
    rows: Option<Vec<String>>,
    cols: Option<Vec<String>>,
    /// `names(dimnames)[1]`; `Some` even when empty (GNU `rn != NULL`).
    row_title: Option<String>,
    /// `names(dimnames)[2]`; printed above the first column.
    col_title: Option<String>,
}

fn empty_matrix_dimnames() -> MatrixDimnames {
    MatrixDimnames {
        rows: None,
        cols: None,
        row_title: None,
        col_title: None,
    }
}

/// GNU `GetMatrixDimnames`: `names(dimnames)` is a STRSXP; empty
/// strings stay present so printarray still emits the title row.
fn matrix_dimname_titles(dimnames: SEXP) -> (Option<String>, Option<String>) {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            dimnames,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if names.is_null()
            || names == R_NilValue()
            || TYPEOF(names) != SEXPTYPE::STRSXP
            || XLENGTH(names) < 2
        {
            return (None, None);
        }
        let elt = |i: R_xlen_t| -> String {
            let s = STRING_ELT(names, i);
            if s.is_null() || s == R_NilValue() {
                return String::new();
            }
            let p = CHAR(s);
            if p.is_null() {
                return String::new();
            }
            std::ffi::CStr::from_ptr(p)
                .to_string_lossy()
                .into_owned()
        };
        (Some(elt(0)), Some(elt(1)))
    }
}

fn matrix_dimnames(x: Sexp<'_>, nrow: usize, ncol: usize) -> MatrixDimnames {
    unsafe {
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        let Some(dimnames) = Sexp::from_raw(dimnames) else {
            return empty_matrix_dimnames();
        };
        if dimnames.clone().typeof_() != SEXPTYPE::VECSXP || dimnames.clone().len() < 2 {
            return empty_matrix_dimnames();
        }
        let (row_title, col_title) = matrix_dimname_titles(dimnames.clone().as_raw());
        let row_names = string_vector_values(crate::sexp::accessors::VECTOR_ELT(
            dimnames.clone().as_raw(),
            0,
        ))
        .filter(|names| names.len() == nrow);
        let col_names =
            string_vector_values(crate::sexp::accessors::VECTOR_ELT(dimnames.as_raw(), 1))
                .filter(|names| names.len() == ncol);
        MatrixDimnames {
            rows: row_names,
            cols: col_names,
            row_title,
            col_title,
        }
    }
}

/// GNU `init_rl_rn`: a present row title (even `""`) adds `R_MIN_LBLOFF`.
const R_MIN_LBLOFF: usize = 2;

fn matrix_row_geometry(row_labels: &[String], row_title: Option<&str>) -> (usize, usize) {
    let mut row_width = row_labels.iter().map(String::len).max().unwrap_or(0);
    let mut lbloff = 0;
    if let Some(title) = row_title {
        let rnw = title.len();
        lbloff = if rnw < row_width + R_MIN_LBLOFF {
            R_MIN_LBLOFF
        } else {
            rnw.saturating_sub(row_width)
        };
        row_width += lbloff;
    }
    (row_width, lbloff)
}

fn gnu_row_index_label(row: usize, nrow: usize) -> String {
    let rlabw = ((nrow as f64).log10().floor() as usize) + 1 + 3;
    format!("{:>rlabw$}", format!("[{},]", row + 1))
}

fn format_real_matrix_gnu(x: Sexp<'_>, nrow: usize, ncol: usize) -> String {
    unsafe {
        use crate::mainutils::format::formatReal;
        use crate::mainutils::printutils::EncodeReal;
        use crate::sexp::accessors::REAL;
        let data = REAL(x.clone().as_raw());
        let mut col_fmt = Vec::with_capacity(ncol);
        for c in 0..ncol {
            let mut w = 0;
            let mut d = 0;
            let mut e = 0;
            formatReal(
                data.add(c * nrow),
                nrow as R_xlen_t,
                &mut w,
                &mut d,
                &mut e,
                0,
            );
            col_fmt.push((w, d, e));
        }
        format_matrix_with(x, nrow, ncol, |r, c| {
            let (w, d, e) = col_fmt[c];
            let encoded = EncodeReal(*data.add(r + c * nrow), w, d, e, b'.' as _);
            if encoded.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(encoded)
                    .to_string_lossy()
                    .into_owned()
            }
        })
    }
}

fn print_max_cells() -> usize {
    unsafe {
        let extras = print_dispatch_extras();
        let mut cur = extras;
        while !cur.is_null() && cur != R_NilValue() {
            if printable_attribute_name(cur).as_deref() == Some("max") {
                let value = CAR(cur);
                if !value.is_null() && value != R_NilValue() {
                    if TYPEOF(value) == SEXPTYPE::INTSXP && XLENGTH(value) > 0 {
                        let n = *crate::sexp::accessors::INTEGER(value);
                        if n > 0 {
                            return n as usize;
                        }
                    } else if TYPEOF(value) == SEXPTYPE::REALSXP && XLENGTH(value) > 0 {
                        let n = *crate::sexp::accessors::REAL(value);
                        if n.is_finite() && n > 0.0 {
                            return n as usize;
                        }
                    }
                }
            }
            cur = CDR(cur);
        }
        crate::mainutils::options::GetOptionMaxPrint().max(0) as usize
    }
}

/// GNU `print.default(..., quote=)` default TRUE; NA becomes TRUE.
fn print_quote_flag() -> bool {
    unsafe {
        let extras = print_dispatch_extras();
        let mut cur = extras;
        while !cur.is_null() && cur != R_NilValue() {
            if printable_attribute_name(cur).as_deref() == Some("quote") {
                let value = CAR(cur);
                if !value.is_null() && value != R_NilValue() {
                    let q = crate::mainutils::coerce::asLogical(value);
                    if q != crate::sexp::ffi::NA_LOGICAL {
                        return q != 0;
                    }
                }
                return true;
            }
            cur = CDR(cur);
        }
        true
    }
}



fn matrix_print_window(nrow: usize, ncol: usize, max: usize) -> (usize, usize) {
    let c_pr = ncol.min(max);
    let mut r_pr = nrow;
    if ncol > 0 && max / ncol < nrow {
        r_pr = max / ncol;
    }
    if ncol > c_pr && r_pr < 1 && nrow > 0 {
        r_pr = 1;
    }
    (r_pr, c_pr)
}

fn matrix_omitted_message(nrow: usize, ncol: usize, r_pr: usize, c_pr: usize) -> Option<String> {
    if r_pr >= nrow && c_pr >= ncol {
        return None;
    }
    let mut msg = String::from(" [ reached 'max' / getOption(\"max.print\") -- omitted");
    if r_pr < nrow {
        let omitted = nrow - r_pr;
        msg.push_str(&format!(
            " {} row{}",
            omitted,
            if omitted == 1 { "" } else { "s" }
        ));
    }
    if c_pr < ncol {
        if r_pr < nrow {
            msg.push_str(" and");
        }
        let omitted = ncol - c_pr;
        msg.push_str(&format!(
            " {} column{}",
            omitted,
            if omitted == 1 { "" } else { "s" }
        ));
    }
    msg.push_str(" ]");
    Some(msg)
}

fn format_matrix_with<F>(x: Sexp<'_>, nrow: usize, ncol: usize, value_at: F) -> String
where
    F: Fn(usize, usize) -> String,
{
    let max = print_max_cells();
    let (r_pr, c_pr) = matrix_print_window(nrow, ncol, max);
    let dn = matrix_dimnames(x, nrow, ncol);
    let row_labels: Vec<String> = (0..r_pr)
        .map(|r| {
            dn.rows
                .as_ref()
                .and_then(|names| names.get(r))
                .cloned()
                .unwrap_or_else(|| gnu_row_index_label(r, nrow))
        })
        .collect();
    let col_labels: Vec<String> = (0..c_pr)
        .map(|c| {
            dn.cols
                .as_ref()
                .and_then(|names| names.get(c))
                .cloned()
                .unwrap_or_else(|| format!("[,{}]", c + 1))
        })
        .collect();
    let (row_width, lbloff) = matrix_row_geometry(&row_labels, dn.row_title.as_deref());
    let mut values = vec![vec![String::new(); c_pr]; r_pr];
    let mut widths = Vec::with_capacity(c_pr);
    for c in 0..c_pr {
        let mut width = col_labels[c].len().max(1);
        for r in 0..r_pr {
            let value = value_at(r, c);
            width = width.max(value.len());
            values[r][c] = value;
        }
        widths.push(width);
    }

    let page_width = unsafe { crate::mainutils::options::GetOptionWidth().max(10) as usize };
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < c_pr {
        let mut used = row_width;
        let mut end = start;
        while end < c_pr {
            let extra = widths[end] + 1;
            if end > start && used + extra > page_width {
                break;
            }
            used += extra;
            end += 1;
        }
        if end == start {
            end += 1;
        }
        blocks.push((start, end));
        start = end;
    }
    if blocks.is_empty() {
        blocks.push((0, 0));
    }

    let mut lines = Vec::new();
    if r_pr == 0 {
        let mut header = "     ".to_string();
        for c in 0..c_pr {
            if c > 0 {
                header.push(' ');
            }
            header.push_str(&format!("{:>width$}", col_labels[c], width = widths[c]));
        }
        lines.push(header);
    } else {
        for &(cs, ce) in &blocks {
            if let Some(cn) = dn.col_title.as_deref() {
                lines.push(format!("{:row_width$}{cn}", ""));
            }
            let mut header = if let Some(rn) = dn.row_title.as_deref() {
                format!("{rn:<row_width$}")
            } else {
                " ".repeat(row_width)
            };
            if row_width > 0 && cs < ce {
                header.push(' ');
            }
            for c in cs..ce {
                header.push_str(&format!("{:>width$}", col_labels[c], width = widths[c]));
                if c + 1 < ce {
                    header.push(' ');
                }
            }
            lines.push(header);
            for r in 0..r_pr {
                let label = format!("{:lbloff$}{}", "", row_labels[r]);
                let mut line = format!("{label:<row_width$}");
                for c in cs..ce {
                    line.push(' ');
                    line.push_str(&format!("{:>width$}", values[r][c], width = widths[c]));
                }
                lines.push(line);
            }
        }
    }

    if let Some(omitted) = matrix_omitted_message(nrow, ncol, r_pr, c_pr) {
        lines.push(omitted);
    }
    lines.join("\n")
}



fn format_character_matrix_with<F>(x: Sexp<'_>, nrow: usize, ncol: usize, value_at: F) -> String
where
    F: Fn(usize, usize) -> String,
{
    let dn = matrix_dimnames(x, nrow, ncol);
    let row_labels: Vec<String> = (0..nrow)
        .map(|r| {
            dn.rows
                .as_ref()
                .and_then(|names| names.get(r))
                .cloned()
                .unwrap_or_else(|| format!("[{},]", r + 1))
        })
        .collect();
    let col_labels: Vec<String> = (0..ncol)
        .map(|c| {
            dn.cols
                .as_ref()
                .and_then(|names| names.get(c))
                .cloned()
                .unwrap_or_else(|| format!("[,{}]", c + 1))
        })
        .collect();
    let (row_width, lbloff) = matrix_row_geometry(&row_labels, dn.row_title.as_deref());
    let empty_row_labs = row_labels.iter().all(|s| s.is_empty());
    let mut values = vec![vec![String::new(); ncol]; nrow];
    let mut widths = Vec::with_capacity(ncol);
    for c in 0..ncol {
        let mut width = col_labels[c].len().max(1);
        for r in 0..nrow {
            let value = value_at(r, c);
            width = width.max(value.len());
            values[r][c] = value;
        }
        widths.push(width);
    }

    let page_width = unsafe {
        crate::mainutils::options::GetOptionWidth().max(10) as usize
    };
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < ncol {
        let mut used = row_width;
        let mut end = start;
        while end < ncol {
            let extra = widths[end] + if row_width > 0 || end > start || empty_row_labs {
                1
            } else {
                0
            };
            if end > start && used + extra > page_width {
                break;
            }
            used += extra;
            end += 1;
        }
        if end == start {
            end += 1;
        }
        blocks.push((start, end));
        start = end;
    }

    let mut lines = Vec::new();
    for &(cs, ce) in &blocks {
        if let Some(cn) = dn.col_title.as_deref() {
            lines.push(format!("{:row_width$}{cn}", ""));
        }

        let mut header = if let Some(rn) = dn.row_title.as_deref() {
            format!("{rn:<row_width$}")
        } else {
            " ".repeat(row_width)
        };
        if row_width > 0 {
            header.push(' ');
        }
        for c in cs..ce {
            if c > cs || (row_width == 0 && empty_row_labs) {
                // leading space is added per cell on data rows; header
                // matches once the first empty row-label space is emitted.
            }
            if c > cs || row_width > 0 {
                if c > cs {
                    header.push(' ');
                }
            } else if empty_row_labs {
                header.push(' ');
            }
            // GNU printStringMatrix uses LeftMatrixColumnLabel.
            header.push_str(&format!("{:<width$}", col_labels[c], width = widths[c]));

        }
        lines.push(header);

        for r in 0..nrow {
            let label = format!("{:lbloff$}{}", "", row_labels[r]);
            let mut line = format!("{label:<row_width$}");
            for c in cs..ce {
                line.push(' ');
                line.push_str(&format!("{:<width$}", values[r][c], width = widths[c]));
            }
            lines.push(line);
        }
    }
    lines.join("\n")
}


fn format_complex_matrix_gnu(x: Sexp<'_>, nrow: usize, ncol: usize) -> String {
    unsafe {
        use crate::mainutils::format::formatComplex;
        use crate::mainutils::printutils::EncodeComplex;
        use crate::sexp::accessors::COMPLEX;
        let data = COMPLEX(x.clone().as_raw());
        let mut col_fmt = Vec::with_capacity(ncol);
        for c in 0..ncol {
            let mut wr = 0;
            let mut dr = 0;
            let mut er = 0;
            let mut wi = 0;
            let mut di = 0;
            let mut ei = 0;
            formatComplex(
                data.add(c * nrow),
                nrow as R_xlen_t,
                &mut wr,
                &mut dr,
                &mut er,
                &mut wi,
                &mut di,
                &mut ei,
                0,
            );
            col_fmt.push((wr, dr, er, wi, di, ei));
        }
        format_matrix_with(x, nrow, ncol, |r, c| {
            let (wr, dr, er, wi, di, ei) = col_fmt[c];
            let encoded = EncodeComplex(
                *data.add(r + c * nrow),
                wr,
                dr,
                er,
                wi,
                di,
                ei,
                b".\0".as_ptr() as *const std::os::raw::c_char,
            );
            if encoded.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(encoded)
                    .to_string_lossy()
                    .into_owned()
            }
        })
    }
}


fn format_matrix(x: Sexp<'_>) -> Option<String> {
    let Some((nrow, ncol)) = matrix_dims(x.clone()) else {
        return format_array(x);
    };
    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => Some(format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_integer_element(x.clone(), (r + c * nrow) as i64)
        })),
        SEXPTYPE::REALSXP => Some(format_real_matrix_gnu(x.clone(), nrow, ncol)),
        SEXPTYPE::LGLSXP => Some(format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_logical_element(x.clone(), (r + c * nrow) as i64)
        })),
        SEXPTYPE::CPLXSXP => Some(format_complex_matrix_gnu(x.clone(), nrow, ncol)),
        SEXPTYPE::STRSXP => {
            let quote = print_quote_flag()
                && !has_class(x.clone(), "noquote")
                && !has_class(x.clone(), "table");
            Some(format_character_matrix_with(
                x.clone(),
                nrow,
                ncol,
                |r, c| format_string_element_maybe_quoted(x.clone(), (r + c * nrow) as i64, quote),
            ))
        }

        _ => None,
    }
}


fn array_dims(x: Sexp<'_>) -> Option<Vec<usize>> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim = Sexp::from_raw(dim)?;
        if dim.clone().typeof_() != SEXPTYPE::INTSXP || dim.clone().len() < 3 {
            return None;
        }
        let mut dims = Vec::with_capacity(dim.clone().len() as usize);
        let mut product = 1usize;
        for i in 0..dim.clone().len() {
            let n = dim.clone().integer_elt(i)? as usize;
            product = product.checked_mul(n)?;
            dims.push(n);
        }
        if product > x.len() as usize {
            return None;
        }
        Some(dims)
    }
}

fn ceil_div(a: usize, b: usize) -> usize {
    if b == 0 {
        0
    } else {
        a.div_ceil(b)
    }
}

fn format_array_slice<F>(label_nrow: usize, use_nr: usize, use_nc: usize, value_at: F) -> String
where
    F: Fn(usize, usize) -> String,
{
    let row_labels: Vec<String> = (0..use_nr)
        .map(|r| gnu_row_index_label(r, label_nrow.max(1)))
        .collect();
    let col_labels: Vec<String> = (0..use_nc).map(|c| format!("[,{}]", c + 1)).collect();
    let (row_width, lbloff) = matrix_row_geometry(&row_labels, None);
    let mut values = vec![vec![String::new(); use_nc]; use_nr];
    let mut widths = Vec::with_capacity(use_nc);
    for c in 0..use_nc {
        let mut width = col_labels[c].len().max(1);
        for r in 0..use_nr {
            let value = value_at(r, c);
            width = width.max(value.len());
            values[r][c] = value;
        }
        widths.push(width);
    }
    let mut lines = Vec::new();
    let mut header = " ".repeat(row_width);
    if row_width > 0 && use_nc > 0 {
        header.push(' ');
    }
    for c in 0..use_nc {
        header.push_str(&format!("{:>width$}", col_labels[c], width = widths[c]));
        if c + 1 < use_nc {
            header.push(' ');
        }
    }
    lines.push(header);
    for r in 0..use_nr {
        let label = format!("{:lbloff$}{}", "", row_labels[r]);
        let mut line = format!("{label:<row_width$}");
        for c in 0..use_nc {
            line.push(' ');
            line.push_str(&format!("{:>width$}", values[r][c], width = widths[c]));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn format_array(x: Sexp<'_>) -> Option<String> {
    let dims = array_dims(x.clone())?;
    let nr = dims[0];
    let nc = dims[1];
    let b = nr.saturating_mul(nc);
    let mut nb = 1usize;
    for &d in &dims[2..] {
        nb = nb.saturating_mul(d);
    }
    let max = print_max_cells();
    let max_reached = b > 0 && nb > 0 && max / b < nb;
    let (nb_pr, nc_last, nr_last) = if max_reached {
        let mut nb_pr = ceil_div(max, b);
        let ne_last = max.saturating_sub(b.saturating_mul(nb_pr.saturating_sub(1)));
        let mut nc_last = ne_last.min(nc);
        let mut nr_last = if ne_last < nc { 1 } else { ne_last / nc };
        if nr_last == 0 {
            nb_pr = nb_pr.saturating_sub(1);
            nc_last = nc;
            nr_last = nr;
        }
        (nb_pr.max(1), nc_last, nr_last)
    } else {
        (nb.max(1), nc, nr)
    };

    let value_at = |offset: usize, r: usize, c: usize| -> String {
        let index = (offset + r + c * nr) as i64;
        match x.clone().typeof_() {
            SEXPTYPE::LGLSXP => format_logical_element(x.clone(), index),
            SEXPTYPE::INTSXP => format_integer_element(x.clone(), index),
            SEXPTYPE::REALSXP => format_real_element(x.clone(), index),
            SEXPTYPE::CPLXSXP => format_complex_element(x.clone(), index),
            SEXPTYPE::STRSXP => {
                format_string_element_maybe_quoted(x.clone(), index, true)
            }
            _ => "NA".to_string(),
        }
    };

    let mut sections = Vec::new();
    for ii in 0..nb_pr {
        let i_last = ii + 1 == nb_pr;
        let use_nc = if i_last { nc_last } else { nc };
        let use_nr = if i_last { nr_last } else { nr };
        let mut header = String::from(", ");
        let mut k = 1usize;
        for &extent in &dims[2..] {
            let l = if extent == 0 {
                1
            } else {
                (ii / k) % extent + 1
            };
            header.push_str(&format!(", {l}"));
            k = k.saturating_mul(extent.max(1));
        }
        let offset = ii.saturating_mul(b);
        let body = format_array_slice(nr, use_nr, use_nc, |r, c| value_at(offset, r, c));
        sections.push(format!("{header}\n\n{body}\n\n"));
    }
    if max_reached {
        let mut msg =
            String::from(" [ reached 'max' / getOption(\"max.print\") -- omitted");
        if nb_pr < nb {
            let omitted = nb - nb_pr;
            msg.push_str(&format!(
                " {} slice{}",
                omitted,
                if omitted == 1 { "" } else { "s" }
            ));
        } else if nb_pr == nb {

            let nr_rem = nr.saturating_sub(nr_last);
            if nr_rem > 0 {
                msg.push_str(&format!(
                    " {} row{}",
                    nr_rem,
                    if nr_rem == 1 { "" } else { "s" }
                ));
            }
            let nc_rem = nc.saturating_sub(nc_last);
            if nc_rem > 0 {
                msg.push_str(&format!(
                    " {} column{}",
                    nc_rem,
                    if nc_rem == 1 { "" } else { "s" }
                ));
            }
        }
        msg.push_str(" ] ");
        sections.push(msg);
    }
    Some(sections.join(""))
}




fn factor_levels(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "factor") {
            return None;
        }

        let levels = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_LevelsSymbol(),
        );
        string_vector_labels(levels).filter(|levels| !levels.is_empty())

    }
}

fn string_vector_contains(x: SEXP, needle: &str) -> bool {
    string_vector_values(x)
        .map(|values| values.iter().any(|value| value == needle))
        .unwrap_or(false)
}

fn string_vector_values(x: SEXP) -> Option<Vec<String>> {
    let sexp = Sexp::from_raw(x)?;
    if sexp.clone().typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.clone().len() as usize);
    for i in 0..sexp.clone().len() {
        values.push(string_element_text(sexp.clone(), i).flatten()?.to_string());
    }
    Some(values)
}

fn string_vector_labels(x: SEXP) -> Option<Vec<String>> {
    let sexp = Sexp::from_raw(x)?;
    if sexp.clone().typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.clone().len() as usize);
    for i in 0..sexp.clone().len() {
        values.push(match string_element_text(sexp.clone(), i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "<NA>".to_string(),
        });
    }
    Some(values)
}

fn vector_print_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = string_vector_labels(names)?;
        if names.len() != x.len() as usize || names.iter().all(|name| name.is_empty()) {
            None
        } else {
            Some(names)
        }
    }
}

fn has_names_attribute(x: Sexp<'_>) -> bool {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        Sexp::from_raw(names).is_some_and(|names| names.typeof_() == SEXPTYPE::STRSXP)
    }
}

fn format_named_atomic_vector(x: Sexp<'_>, values: Vec<String>) -> Option<String> {
    let mut names = vector_print_names(x)?;
    let limit = values.len();
    names.truncate(limit);
    Some(format_named_values(&names, &values))
}

fn string_element_text<'a>(x: Sexp<'a>, i: R_xlen_t) -> Option<Option<&'a str>> {
    x.string_text_elt(i)
}

fn format_string_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    format_string_element_maybe_quoted(x, i, true)
}

fn format_string_element_maybe_quoted(x: Sexp<'_>, i: R_xlen_t, quote: bool) -> String {
    match string_element_text(x, i) {
        Some(Some(value)) if quote => format!("\"{}\"", escape_printed_string(value)),
        Some(Some(value)) => value.to_string(),
        Some(None) | None => "NA".to_string(),
    }
}


fn escape_printed_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_string_vector(x: Sexp<'_>) -> String {
    if x.clone().len() == 0 {
        return "character(0)".to_string();
    }
    let vals: Vec<String> = (0..x.clone().len().min(10))
        .map(|i| format_string_element(x.clone(), i))
        .collect();
    let suffix = if x.len() > 10 { " ..." } else { "" };
    format!("[1] {}{}", vals.join(" "), suffix)
}

fn format_date_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_real_elt(i)
        .ok()
        .and_then(crate::mainutils::essentials::date_days_to_iso)
        .map(|value| format!("\"{}\"", escape_printed_string(&value)))
        .unwrap_or_else(|| "NA".to_string())
}

/// GNU `print.Date`: `print(format(x))` so width wrapping matches print.default.
fn format_date_vector(x: Sexp<'_>) -> String {
    format_date_vector_max(x, None)
}

pub(crate) fn format_date_vector_max(x: Sexp<'_>, max_override: Option<i64>) -> String {
    if x.clone().len() == 0 {
        return "Date of length 0".to_string();
    }
    unsafe {
        let n = x.clone().len();
        let opt_max = crate::mainutils::options::GetOptionMaxPrint() as i64;
        let max = max_override.unwrap_or(opt_max).max(0);
        let n_show = n.min(max as R_xlen_t);
        let formatted = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, n_show);
        if formatted.is_null() {
            return String::new();
        }
        let _g = crate::sexp::protect::protect(formatted);
        for i in 0..n_show {
            let days = x
                .clone()
                .try_real_elt(i)
                .ok()
                .or_else(|| {
                    x.clone()
                        .try_integer_elt(i)
                        .ok()
                        .and_then(|v| {
                            if v == crate::sexp::NA_INTEGER {
                                None
                            } else {
                                Some(v as f64)
                            }
                        })
                });
            let text = days
                .and_then(crate::mainutils::essentials::date_days_to_iso)
                .unwrap_or_else(|| "NA".to_string());

            let c = std::ffi::CString::new(text).unwrap_or_default();
            crate::sexp::accessors::SET_STRING_ELT(
                formatted,
                i,
                crate::sexp::constructors::Rf_mkChar(c.as_ptr()),
            );
        }
        let sexp = Sexp::from_raw_unchecked(formatted);
        // GNU print.Date uses max+1 when truncating so print.default does
        // not emit a second omitted line.
        let print_max = if n_show < n { n_show as i64 + 1 } else { max };
        let mut out = format_vector_stock_n(sexp, true, Some(print_max));
        if n_show < n {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]",
                n - n_show
            ));
        }
        out
    }
}


fn format_posixct_element(x: Sexp<'_>, i: R_xlen_t, include_tz: bool, force_time: bool) -> String {
    x.try_real_elt(i)
        .ok()
        .and_then(|seconds| {
            crate::mainutils::essentials::posix_seconds_to_iso_with_time(
                seconds, include_tz, force_time,
            )
        })
        .map(|value| format!("\"{}\"", escape_printed_string(&value)))
        .unwrap_or_else(|| "NA".to_string())
}

fn posixct_vector_needs_time(x: Sexp<'_>) -> bool {
    (0..x.clone().len()).any(|i| {
        x.clone().try_real_elt(i).ok().is_some_and(|seconds| {
            !R_IsNA(seconds) && seconds.is_finite() && seconds.floor() as i64 % 86_400 != 0
        })
    })
}

fn format_posixct_vector(x: Sexp<'_>, include_tz: bool) -> String {
    format_posixct_vector_max(x, include_tz, None)
}

pub(crate) fn format_posixct_vector_max(
    x: Sexp<'_>,
    include_tz: bool,
    max_override: Option<i64>,
) -> String {
    if x.clone().len() == 0 {
        return "POSIXct of length 0".to_string();
    }
    unsafe {
        use crate::sexp::constructors::{Rf_ScalarLogical, Rf_cons, Rf_mkString};
        use crate::sexp::ffi::TRUE;
        use crate::sexp::protect::protect;
        let n = x.clone().len();
        let opt_max = crate::mainutils::options::GetOptionMaxPrint() as i64;
        let max = max_override.unwrap_or(opt_max).max(0);
        let format = Rf_mkString(c"".as_ptr());
        let _fmt = protect(format);
        let usetz = Rf_ScalarLogical(if include_tz { TRUE } else { 0 });
        let _u = protect(usetz);
        let args = Rf_cons(
            x.as_raw(),
            Rf_cons(format, Rf_cons(R_NilValue(), Rf_cons(usetz, R_NilValue()))),
        );
        let _a = protect(args);
        let formatted = crate::mainutils::datetime::do_format_POSIXct(
            R_NilValue(),
            R_NilValue(),
            args,
            crate::sexp::globals::R_BaseEnv(),
        );
        if formatted.is_null() {
            return String::new();
        }
        let _f = protect(formatted);
        let n_show = XLENGTH(formatted).min(n.min(max as R_xlen_t));
        let sexp = Sexp::from_raw_unchecked(formatted);
        let print_max = if n_show < n { n_show as i64 + 1 } else { max };
        let mut out = format_vector_stock_n(sexp, true, Some(print_max));
        if n_show < n {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]",
                n - n_show
            ));
        }
        out
    }
}


fn posixlt_time_length(x: Sexp<'_>) -> R_xlen_t {
    unsafe {
        let raw = x.as_raw();
        let ncomp = XLENGTH(raw);
        let mut n = 0;
        for i in 0..ncomp {
            let col = VECTOR_ELT(raw, i);
            if !col.is_null() {
                n = n.max(XLENGTH(col));
            }
        }
        n
    }
}

fn format_posixlt_vector(x: Sexp<'_>) -> String {
    let n = posixlt_time_length(x.clone());
    if n == 0 {
        return "POSIXlt of length 0".to_string();
    }
    unsafe {
        use crate::sexp::constructors::{
            Rf_ScalarInteger, Rf_ScalarLogical, Rf_cons, Rf_mkString,
        };
        use crate::sexp::ffi::{NA_INTEGER, TRUE};
        use crate::sexp::protect::protect;
        let format = Rf_mkString(c"".as_ptr());
        let _fmt = protect(format);
        let usetz = Rf_ScalarLogical(TRUE);
        let _u = protect(usetz);
        let digits = Rf_ScalarInteger(NA_INTEGER);
        let _d = protect(digits);
        let args = Rf_cons(
            x.as_raw(),
            Rf_cons(format, Rf_cons(usetz, Rf_cons(digits, R_NilValue()))),
        );
        let _a = protect(args);
        let formatted = crate::mainutils::datetime::do_format_POSIXlt(
            R_NilValue(),
            R_NilValue(),
            args,
            crate::sexp::globals::R_BaseEnv(),
        );
        if formatted.is_null() {
            return String::new();
        }
        let _f = protect(formatted);
        format_vector_stock_n(Sexp::from_raw_unchecked(formatted), true, None)
    }
}




fn difftime_units(x: Sexp<'_>) -> String {
    unsafe {
        let units = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
        );
        if let Some(units) = Sexp::from_raw(units)
            && units.clone().typeof_() == SEXPTYPE::STRSXP
            && units.clone().len() > 0
            && let Some(Some(value)) = string_element_text(units, 0)
        {
            return value.to_string();
        }
        "secs".to_string()
    }
}

fn format_difftime_vector(x: Sexp<'_>) -> String {
    let units = difftime_units(x.clone());
    if x.clone().len() == 0 {
        return format!("Time difference of 0 {units}");
    }
    let value = x
        .try_real_elt(0)
        .map(format_real_value)
        .unwrap_or_else(format_access_error);
    format!("Time difference of {value} {units}")
}

fn format_factor(x: Sexp<'_>) -> Option<String> {
    let levels = factor_levels(x.clone())?;
    let mut vals: Vec<String> = x
        .clone()
        .iter_integer()
        .take(10)
        .map(|code| {
            if code == NA_INTEGER {
                "<NA>".to_string()
            } else {
                levels
                    .get((code - 1) as usize)
                    .cloned()
                    .unwrap_or_else(|| code.to_string())
            }
        })
        .collect();
    let width = vals.iter().map(|s| s.chars().count()).max().unwrap_or(0);
    for val in &mut vals {
        *val = format!("{val:<width$}");
    }
    let suffix = if x.len() > 10 { " ..." } else { "" };
    Some(format!(
        "[1] {}{}\nLevels: {}",
        vals.join(" "),
        suffix,
        levels.join(" ")
    ))
}

fn table_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "table") {
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if let Some(labels) =
            string_vector_values(names).filter(|names| names.len() == x.len() as usize)
        {
            return Some(labels);
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if dimnames.is_null() || TYPEOF(dimnames) != SEXPTYPE::VECSXP || XLENGTH(dimnames) < 1 {
            return None;
        }
        string_vector_values(VECTOR_ELT(dimnames, 0))
            .filter(|names| names.len() == x.len() as usize)
    }
}

fn table_title(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let title = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"table.name".as_ptr()),
        );
        let title = Sexp::from_raw(title)?;
        if title.clone().typeof_() != SEXPTYPE::STRSXP || title.clone().len() == 0 {
            return None;
        }
        string_element_text(title, 0).flatten().map(str::to_string)
    }
}

fn format_table(x: Sexp<'_>) -> Option<String> {
    let names = table_names(x.clone())?;
    let values: Vec<String> = match x.clone().typeof_() {
        SEXPTYPE::INTSXP => (0..x.clone().len())
            .map(|i| format_integer_element(x.clone(), i))
            .collect(),
        SEXPTYPE::REALSXP => (0..x.clone().len())
            .map(|i| format_real_element(x.clone(), i))
            .collect(),
        _ => return None,
    };
    if let Some(title) = table_title(x) {
        let widths: Vec<usize> = names
            .iter()
            .zip(&values)
            .map(|(name, value)| name.len().max(value.len()).max(4))
            .collect();
        let name_line = names
            .iter()
            .zip(&widths)
            .map(|(name, width)| format!("{name:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        let value_line = values
            .iter()
            .zip(&widths)
            .map(|(value, width)| format!("{value:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        Some(format!("{title}\n{name_line}\n{value_line}"))
    } else {
        let width = names
            .iter()
            .zip(&values)
            .map(|(name, value)| name.len().max(value.len()))
            .max()
            .unwrap_or(1);
        let name_line = names
            .iter()
            .map(|name| format!("{name:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        let value_line = values
            .iter()
            .map(|value| format!("{value:>width$}"))
            .collect::<Vec<_>>()
            .join(" ");
        // GNU print.array of a 1-d table keeps a trailing column space.
        Some(format!("\n{name_line} \n{value_line} "))
    }
}

fn summary_default_digits() -> i32 {
    unsafe { 3.max(crate::mainutils::options::GetOptionDigits() - 3) }
}

fn with_summary_default_digits<T>(f: impl FnOnce() -> T) -> T {
    unsafe {
        let digits = summary_default_digits();
        let old = crate::mainutils::format::format_get_R_print();
        let previous = crate::mainutils::format::format_set_R_print(
            crate::mainutils::format::RPrint {
                digits,
                scipen: old.scipen,
                na_width: old.na_width,
                na_width_noquote: old.na_width_noquote,
            },
        );
        let rendered = f();
        crate::mainutils::format::format_set_R_print(previous);
        rendered
    }
}

fn format_named_summary_reals(x: Sexp<'_>, names: &[String]) -> Option<Vec<String>> {
    unsafe {
        let slice = x.as_real_slice()?;
        if slice.len() != names.len() {
            return None;
        }
        let mut finite = Vec::new();
        let mut nas_at = None;
        for (i, name) in names.iter().enumerate() {
            if name == "NAs" {
                nas_at = Some(i);
            } else {
                finite.push(i);
            }
        }
        let n = finite.len() as R_xlen_t;
        let tmp = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if tmp.is_null() {
            return None;
        }
        let _tmp = crate::sexp::protect::protect(tmp);
        for (j, &i) in finite.iter().enumerate() {
            *crate::sexp::accessors::REAL(tmp).add(j) = slice[i];
        }
        Some(with_summary_default_digits(|| {
            let mut w = 0;
            let mut d = 0;
            let mut e = 0;
            crate::mainutils::format::formatRealS(tmp, n, &mut w, &mut d, &mut e, 0);
            let mut values = vec![String::new(); names.len()];
            for (j, &i) in finite.iter().enumerate() {
                values[i] = encode_cstr(crate::mainutils::printutils::EncodeReal0(
                    crate::sexp::accessors::REAL_ELT(tmp, j as std::os::raw::c_int),
                    w,
                    d,
                    e,
                    OUT_DEC,
                ));
            }
            if let Some(i) = nas_at {
                values[i] = format!("{}", slice[i] as i64);
            }
            values
        }))
    }
}



fn format_summary_default_unnamed_numeric(x: Sexp<'_>) -> String {
    with_summary_default_digits(|| unsafe { format_vector_stock(x, false) })
}



fn format_summary_default(x: Sexp<'_>) -> Option<String> {
    if !has_class(x.clone(), "summaryDefault") || !has_class(x.clone(), "table") {
        return None;
    }
    let Some(names) = table_names(x.clone()) else {
        if x.clone().typeof_() != SEXPTYPE::REALSXP {
            return None;
        }
        return Some(format_summary_default_unnamed_numeric(x));
    };
    let values: Vec<String> = match x.clone().typeof_() {
        SEXPTYPE::REALSXP => format_named_summary_reals(x.clone(), &names)?,


        SEXPTYPE::INTSXP => (0..x.clone().len())
            .map(|i| {
                if matches!(
                    names.get(i as usize).map(String::as_str),
                    Some("Min.nchar" | "Max.nchar")
                ) && format_integer_element(x.clone(), i) == "NA"
                {
                    String::new()
                } else {
                    format_integer_element(x.clone(), i)
                }
            })
            .collect(),
        SEXPTYPE::STRSXP => (0..x.clone().len())
            .map(|i| match string_element_text(x.clone(), i) {
                Some(Some(value)) => value.to_string(),
                Some(None) | None => "NA".to_string(),
            })
            .collect(),
        _ => return None,
    };
    let min_width = match x.typeof_() {
        SEXPTYPE::REALSXP | SEXPTYPE::STRSXP => 7,
        SEXPTYPE::INTSXP => 8,
        _ => 0,
    };
    let widths: Vec<usize> = names
        .iter()
        .zip(&values)
        .map(|(name, value)| name.len().max(value.len()).max(min_width))
        .collect();
    let name_line = names
        .iter()
        .zip(&widths)
        .map(|(name, width)| format!("{name:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    let value_line = values
        .iter()
        .zip(&widths)
        .map(|(value, width)| format!("{value:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("{name_line} \n{value_line} "))

}

fn list_names(x: Sexp<'_>) -> Vec<String> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        string_vector_values(names).unwrap_or_default()
    }
}

fn has_class(x: Sexp<'_>, class_name: &str) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        string_vector_contains(class, class_name)
    }
}

fn data_frame_nrows(x: Sexp<'_>) -> R_xlen_t {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
        );
        if let Some(row_names) = Sexp::from_raw(row_names)
            && row_names.clone().typeof_() == SEXPTYPE::INTSXP
            && row_names.clone().len() == 2
            && let Some(values) = row_names.as_integer_slice()
            && values[0] == NA_INTEGER
            && values[1] < 0
        {
            return (-values[1]) as R_xlen_t;
        }
    }
    x.iter_vector().map(|col| col.len()).max().unwrap_or(0)
}

fn data_frame_row_labels(x: Sexp<'_>, nrow: R_xlen_t) -> Vec<String> {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
        );
        if let Some(row_names) = Sexp::from_raw(row_names)
            && row_names.clone().typeof_() == SEXPTYPE::STRSXP
            && row_names.clone().len() == nrow
        {
            return (0..nrow)
                .map(|i| match string_element_text(row_names.clone(), i) {
                    Some(Some(value)) => value.to_string(),
                    Some(None) | None => (i + 1).to_string(),
                })
                .collect();
        }
    }
    (1..=nrow).map(|i| i.to_string()).collect()
}


fn format_data_frame_cell(x: Sexp<'_>, row: R_xlen_t) -> String {
    if x.clone().len() == 0 {
        return "NA".to_string();
    }
    let i = row % x.clone().len();
    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => format_integer_element(x, i),
        SEXPTYPE::REALSXP => format_real_element(x, i),
        SEXPTYPE::LGLSXP => format_logical_element(x, i),
        SEXPTYPE::STRSXP => match string_element_text(x, i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "NA".to_string(),
        },
        _ => format_sexp_direct(x),
    }
}

fn format_data_frame_column(col: Sexp<'_>, nrow: R_xlen_t) -> Vec<String> {
    unsafe {
        if matches!(
            col.clone().typeof_(),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::CPLXSXP
        ) {
            let args = crate::sexp::constructors::Rf_cons(col.clone().as_raw(), R_NilValue());

            let _g = crate::sexp::protect::protect(args);
            let formatted = crate::mainutils::essentials::do_format(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                R_NilValue(),
            );
            if let Some(formatted) = Sexp::from_raw(formatted)
                && formatted.clone().typeof_() == SEXPTYPE::STRSXP
            {
                return (0..nrow)
                    .map(|i| match string_element_text(formatted.clone(), i) {
                        Some(Some(value)) => value.to_string(),
                        Some(None) | None => "NA".to_string(),
                    })
                    .collect();
            }
        }
    }
    (0..nrow)
        .map(|row| format_data_frame_cell(col.clone(), row))
        .collect()
}


fn format_data_frame(x: Sexp<'_>) -> Option<String> {
    if !has_class(x.clone(), "data.frame") {
        return None;
    }
    let names = list_names(x.clone());
    let nrow = data_frame_nrows(x.clone());
    let row_labels = data_frame_row_labels(x.clone(), nrow);
    let columns: Vec<Sexp<'_>> = x.iter_vector().collect();
    let row_width = row_labels
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(1)
        .max(1);
    let formatted_cols: Vec<Vec<String>> = columns
        .iter()
        .map(|col| format_data_frame_column(col.clone(), nrow))
        .collect();
    let widths: Vec<usize> = formatted_cols
        .iter()
        .enumerate()
        .map(|(i, cells)| {
            let name_width = names.get(i).map(String::len).unwrap_or(0);
            let value_width = cells.iter().map(String::len).max().unwrap_or(0);
            name_width.max(value_width)
        })
        .collect();
    let header = format!(
        "{} {}",
        " ".repeat(row_width),
        names
            .iter()
            .zip(&widths)
            .map(|(name, width)| format!("{name:>width$}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut lines = Vec::with_capacity(nrow as usize + 1);
    lines.push(header);
    for row in 0..nrow {
        let row_name = format!(
            "{:>row_width$}",
            row_labels
                .get(row as usize)
                .cloned()
                .unwrap_or_else(|| (row + 1).to_string())
        );
        let cells = formatted_cols
            .iter()
            .zip(&widths)
            .map(|(col, width)| {
                format!(
                    "{:>width$}",
                    col.get(row as usize).cloned().unwrap_or_else(|| "NA".to_string())
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("{row_name} {cells}"));
    }
    Some(lines.join("\n"))
}

fn is_reserved_r_name(name: &str) -> bool {
    matches!(
        name,
        "if" | "else"
            | "repeat"
            | "while"
            | "function"
            | "for"
            | "in"
            | "next"
            | "break"
            | "TRUE"
            | "FALSE"
            | "NULL"
            | "Inf"
            | "NaN"
            | "NA"
            | "NA_integer_"
            | "NA_real_"
            | "NA_complex_"
            | "NA_character_"
    )
}

fn is_syntactic_r_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || is_reserved_r_name(name) {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_alphabetic() || first == b'.') {
        return false;
    }
    if first == b'.' && bytes.get(1).is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    bytes
        .iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_')
}

fn list_name_tag(name: &str) -> String {
    if is_syntactic_r_name(name) {
        format!("${name}")
    } else {
        format!("$`{name}`")
    }
}

fn list_element_header(index: usize, names: &[String]) -> String {
    match names.get(index) {
        Some(name) if !name.is_empty() => list_name_tag(name),
        _ => format!("[[{}]]", index + 1),
    }
}

fn format_list(x: Sexp<'_>) -> String {
    format_list_with_path(x, "")
}

fn format_list_with_path(x: Sexp<'_>, path: &str) -> String {
    if x.clone().len() == 0 {
        return format_with_printable_attributes("list()".to_string(), x);
    }
    let names = list_names(x.clone());
    let mut sections = Vec::with_capacity(x.clone().len() as usize);
    for (index, elem) in x.clone().iter_vector().enumerate() {
        let header = format!("{path}{}", list_element_header(index, &names));
        let body = format_list_child(elem, &header);
        sections.push(format!("{header}\n{body}"));
    }
    format_list_body_with_attributes(sections.join("\n\n"), x)
}



fn format_pairlist(x: Sexp<'_>) -> String {
    format_pairlist_with_path(x, "")
}

fn format_pairlist_with_path(x: Sexp<'_>, path: &str) -> String {
    unsafe {
        let mut sections = Vec::new();
        let mut cell = x.clone().as_raw();
        let mut index = 0usize;
        while !cell.is_null() && cell != R_NilValue() && TYPEOF(cell) == SEXPTYPE::LISTSXP {
            let tag = match printable_attribute_name(cell) {
                Some(name) if !name.is_empty() => list_name_tag(&name),
                _ => format!("[[{}]]", index + 1),
            };
            let header = format!("{path}{tag}");
            let body = if let Some(elem) = Sexp::from_raw(CAR(cell)) {
                format_list_child(elem, &header)
            } else {
                "NULL".to_string()
            };
            sections.push(format!("{header}\n{body}"));
            cell = CDR(cell);
            index += 1;
        }

        if sections.is_empty() {
            return format_with_printable_attributes("NULL".to_string(), x);
        }
        format_list_body_with_attributes(sections.join("\n\n"), x)




    }
}

fn format_dispatched_show(raw: crate::sexp::ffi::SEXP) -> Option<String> {
    unsafe {
        let path = crate::mainutils::essentials::find_package_path("methods");
        if crate::mainutils::essentials::cached_namespace_by_name("methods").is_none()
            && !path.is_empty()
        {
            let _ = crate::mainutils::essentials::load_pure_r_package(
                "methods",
                std::path::Path::new(&path),
            );
        }
        let namespace = crate::mainutils::essentials::cached_namespace_by_name("methods")?;
        let symbol = crate::sexp::symbol::Rf_install(c"show".as_ptr());
        let mut fun = crate::sexp::envir::R_findVarInFrame(namespace, symbol);
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::sexp::envir::forcePromise(fun);
        }
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return None;
        }
        let _fun = crate::sexp::protect::protect(fun);
        let call = crate::sexp::constructors::Rf_lang2(fun, raw);
        let _call = crate::sexp::protect::protect(call);
        let env = crate::sexp::globals::R_GlobalEnv();
        let guard = OutputCaptureGuard::start();
        let _ = crate::eval::eval::Rf_eval(call, env);
        let captured = guard.finish();
        Some(captured.stdout.trim_end_matches('\n').to_string())
    }
}


fn format_dispatched_print(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let raw = x.as_raw();
        if crate::sexp::accessors::OBJECT(raw) == 0 {
            return None;
        }
        if crate::mainutils::coerce::IS_S4_OBJECT(raw) != 0 {
            return format_dispatched_show(raw);
        }
        let klass = crate::sexp::attrib_core::getAttrib(
            raw,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let env = crate::sexp::globals::R_GlobalEnv();
        let method = crate::mainutils::objects::lookup_s3_method_for_classes(
            "print", klass, env, env, env, false,
        )?;
        if TYPEOF(method.method) != SEXPTYPE::CLOSXP {
            return None;
        }
        let extras = print_dispatch_extras();
        let extras = if extras.is_null() { R_NilValue() } else { extras };
        let args = crate::sexp::constructors::Rf_cons(raw, extras);
        let _args = crate::sexp::protect::protect(args);
        let print_sym = crate::sexp::symbol::Rf_install(c"print".as_ptr());
        let call = crate::sexp::constructors::Rf_cons(print_sym, args);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let _call = crate::sexp::protect::protect(call);
        let guard = OutputCaptureGuard::start();
        let Some(_) = crate::mainutils::essentials::apply_s3_closure_method(
            "print", call, args, env,
        ) else {
            return None;
        };

        let captured = guard.finish();
        Some(captured.stdout.trim_end_matches('\n').to_string())
    }
}


fn format_list_child(elem: Sexp<'_>, path: &str) -> String {
    if let Some(dispatched) = format_dispatched_print(elem.clone()) {
        return dispatched;
    }
    match elem.clone().typeof_() {
        SEXPTYPE::VECSXP if format_data_frame(elem.clone()).is_none() => {
            format_list_with_path(elem, path)
        }
        SEXPTYPE::LISTSXP => format_pairlist_with_path(elem, path),
        _ => format_sexp_direct(elem),
    }
}


/// Format a value for top-level emission, excluding the caller-owned final
/// line terminator.
///
/// `printList()` emits a separator newline after every non-empty list
/// element, including the last one. Nested lists already include that
/// separator in their body; the outermost list still needs one more so
/// the next top-level print is separated by two blanks.
pub(crate) fn format_sexp_top_level(x: Sexp<'_>) -> String {
    format_sexp_direct(x)
}






fn first_deparse_line(text: crate::sexp::ffi::SEXP) -> Option<String> {
    unsafe {
        if text.is_null() || XLENGTH(text) == 0 {
            return None;
        }
        let charsxp = STRING_ELT(text, 0);
        if charsxp.is_null() {
            return None;
        }
        let chars = CHAR(charsxp);
        if chars.is_null() {
            None
        } else {
            Some(
                std::ffi::CStr::from_ptr(chars)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
}

fn primitive_args_prototype(name: &str) -> Option<crate::sexp::ffi::SEXP> {
    unsafe {
        let name_c = std::ffi::CString::new(name).ok()?;
        let symbol = crate::sexp::symbol::Rf_install(name_c.as_ptr());
        let base = crate::sexp::globals::R_BaseEnv();
        for registry in [".ArgsEnv", ".GenericArgsEnv"] {
            let registry_c = std::ffi::CString::new(registry).ok()?;
            let registry_sym = crate::sexp::symbol::Rf_install(registry_c.as_ptr());
            let mut env = crate::sexp::envir::R_findVarInFrame(base, registry_sym);
            if env.is_null() || env == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(env) == SEXPTYPE::PROMSXP {
                env = crate::eval::eval::Rf_eval(env, base);
            }
            if env.is_null()
                || env == crate::sexp::globals::R_UnboundValue()
                || TYPEOF(env) != SEXPTYPE::ENVSXP
            {
                continue;
            }
            let proto = crate::sexp::envir::R_findVarInFrame(env, symbol);
            if !proto.is_null()
                && proto != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(proto) == SEXPTYPE::CLOSXP
            {
                return Some(proto);
            }
        }
        None
    }
}

fn format_primitive(x: Sexp<'_>) -> String {
    unsafe {
        let name = crate::eval::primitive::PRIMNAME(x.as_raw());
        let primitive = format!(".Primitive(\"{name}\")");
        let Some(proto) = primitive_args_prototype(name) else {
            return primitive;
        };
        let text = crate::mainutils::deparse::deparse1m(
            proto,
            false,
            crate::mainutils::deparse::DEFAULTDEPARSE,
        );
        match first_deparse_line(text) {
            Some(line) if !line.is_empty() => format!("{line} {primitive}"),
            _ => primitive,
        }
    }
}



fn deparse_expression_one(expr: SEXP) -> String {
    unsafe {
        let text = crate::mainutils::deparse::deparse1line(expr, false);
        first_deparse_line(text).unwrap_or_default()
    }
}


fn format_expression_vector(x: Sexp<'_>) -> String {
    unsafe {
        let raw = x.as_raw();
        let n = XLENGTH(raw);
        if n == 0 {
            return "expression()".to_string();
        }
        let parts = (0..n)
            .map(|i| deparse_expression_one(VECTOR_ELT(raw, i)))
            .collect::<Vec<_>>();
        format!("expression({})", parts.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Faithful ports of stock printvector.c: printVector / printNamedVector.
//
// Field widths come from format.rs (formatRealS & co.) and each element is
// encoded through the printutils Encode* primitives at the COMMON width, so
// all elements of a vector share one number of decimals and one field width,
// exactly like stock. Lines wrap at options("width") with "[i]" index labels.
// ---------------------------------------------------------------------------

const OUT_DEC: *const std::os::raw::c_char = b".\0".as_ptr() as *const std::os::raw::c_char;

unsafe fn encode_cstr(p: *const std::os::raw::c_char) -> String {
    // SAFETY: this unsafe function requires `p` to be null or a live,
    // NUL-terminated C string for the duration of the conversion.
    unsafe {
        if p.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// (R_print.width, R_print.gap, R_print.max) from the current options.
fn vector_print_settings() -> (std::os::raw::c_int, std::os::raw::c_int, i64) {
    unsafe {
        let width = crate::mainutils::options::GetOptionWidth();
        let max = crate::mainutils::options::GetOptionMaxPrint();
        let gap = crate::mainutils::printutils::get_R_print().gap;
        (width, gap, max as i64)
    }
}

/// Stock VectorIndex: right-justify "[i]" in `labwidth` columns.
fn vector_index(i: R_xlen_t, labwidth: usize) -> String {
    format!("{:>labwidth$}", format!("[{i}]"))
}

/// The stock type-specific field width `w` (before the gap is added by the
/// callers that add it), computed over the first `n` elements.
unsafe fn type_field_width(
    raw: SEXP,
    tp: SEXPTYPE,
    n: R_xlen_t,
    quote: bool,
) -> (
    std::os::raw::c_int,
    std::os::raw::c_int,
    std::os::raw::c_int,
) {
    // SAFETY: callers provide a live atomic-vector SEXP whose runtime tag is
    // `tp`; the formatting routines only read it and write to stack locals.
    unsafe {
        // (w, d, e); only real/complex use d/e.
        let _ = quote;
        match tp {
            SEXPTYPE::LGLSXP => {
                let mut w = 0;
                crate::mainutils::format::formatLogicalS(raw, n, &mut w);
                (w, 0, 0)
            }
            SEXPTYPE::INTSXP => {
                let mut w = 0;
                crate::mainutils::format::formatIntegerS(raw, n, &mut w);
                (w, 0, 0)
            }
            SEXPTYPE::REALSXP => {
                let mut w = 0;
                let mut d = 0;
                let mut e = 0;
                crate::mainutils::format::formatRealS(raw, n, &mut w, &mut d, &mut e, 0);
                (w, d, e)
            }
            SEXPTYPE::CPLXSXP => {
                let mut wr = 0;
                let mut dr = 0;
                let mut er = 0;
                let mut wi = 0;
                let mut di = 0;
                let mut ei = 0;
                crate::mainutils::format::formatComplexS(
                    raw, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
                );
                (wr + wi + 2, dr, er)
            }
            SEXPTYPE::STRSXP => {
                let mut w = 0;
                crate::mainutils::format::formatStringS(
                    raw,
                    n,
                    &mut w,
                    quote as std::os::raw::c_int,
                );
                (w, 0, 0)
            }
            SEXPTYPE::RAWSXP => {
                let mut w = 0;
                crate::mainutils::format::formatRawS(raw, n, &mut w);
                (w, 0, 0)
            }
            _ => (0, 0, 0),
        }
    }
}

/// formatComplexS results, computed once per vector.
#[derive(Clone, Copy)]
struct ComplexFmt {
    wr: std::os::raw::c_int,
    dr: std::os::raw::c_int,
    er: std::os::raw::c_int,
    wi: std::os::raw::c_int,
    di: std::os::raw::c_int,
    ei: std::os::raw::c_int,
}

unsafe fn complex_fmt(raw: SEXP, n: R_xlen_t) -> ComplexFmt {
    // SAFETY: callers guarantee that `raw` is a live CPLXSXP with at least
    // `n` elements; all output pointers refer to initialized stack locals.
    unsafe {
        let (mut wr, mut dr, mut er, mut wi, mut di, mut ei) = (0, 0, 0, 0, 0, 0);
        crate::mainutils::format::formatComplexS(
            raw, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
        );
        ComplexFmt {
            wr,
            dr,
            er,
            wi,
            di,
            ei,
        }
    }
}

fn part_is_na(v: f64) -> bool {
    v.is_nan() && v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
}

fn part_is_nan(v: f64) -> bool {
    v.is_nan()
}

/// Encode element `i` at the common width, per stock print*Vector tight loops.
unsafe fn encode_element_at(
    raw: SEXP,
    tp: SEXPTYPE,
    i: R_xlen_t,
    w: std::os::raw::c_int,
    d: std::os::raw::c_int,
    e: std::os::raw::c_int,
    quote: bool,
    gap: std::os::raw::c_int,
    cfmt: ComplexFmt,
) -> String {
    // SAFETY: `encode_element_adj` inherits this function's live-SEXP,
    // matching-tag, and in-bounds-index requirements.
    unsafe {
        encode_element_adj(
            raw,
            tp,
            i,
            w,
            d,
            e,
            quote,
            gap,
            cfmt,
            crate::mainutils::printutils::Rprt_adj::right,
        )
    }
}

/// Like encode_element_at, with an explicit justification for STRSXP
/// elements: stock printVector left-adjusts (R_print.right = FALSE by
/// default), printNamedVector right-adjusts (Rprt_adj_right).
unsafe fn encode_element_adj(
    raw: SEXP,
    tp: SEXPTYPE,
    i: R_xlen_t,
    w: std::os::raw::c_int,
    d: std::os::raw::c_int,
    e: std::os::raw::c_int,
    quote: bool,
    gap: std::os::raw::c_int,
    cfmt: ComplexFmt,
    str_adj: crate::mainutils::printutils::Rprt_adj,
) -> String {
    // SAFETY: callers guarantee `raw` is live, `tp` is its actual runtime
    // tag, and `i` is in bounds; formatter-returned C strings are borrowed
    // only until the next formatter call and are copied immediately.
    unsafe {
        let i32i = i as std::os::raw::c_int;
        match tp {
            SEXPTYPE::LGLSXP => encode_cstr(crate::mainutils::printutils::EncodeLogical(
                crate::sexp::accessors::LOGICAL_ELT(raw, i32i),
                w,
            )),
            SEXPTYPE::INTSXP => encode_cstr(crate::mainutils::printutils::EncodeInteger(
                crate::sexp::accessors::INTEGER_ELT(raw, i32i),
                w,
            )),
            SEXPTYPE::REALSXP => encode_cstr(crate::mainutils::printutils::EncodeReal0(
                crate::sexp::accessors::REAL_ELT(raw, i32i),
                w,
                d,
                e,
                OUT_DEC,
            )),
            SEXPTYPE::CPLXSXP => {
                let c = crate::sexp::accessors::COMPLEX_ELT(raw, i32i);
                if part_is_na(c.r) || part_is_na(c.i) {
                    // stock: NA parts render as NA over the total width
                    encode_cstr(crate::mainutils::printutils::EncodeReal0(
                        crate::sexp::ffi::NA_REAL,
                        w,
                        0,
                        0,
                        OUT_DEC,
                    ))
                } else {
                    encode_cstr(crate::mainutils::printutils::EncodeComplex(
                        c,
                        cfmt.wr + gap,
                        cfmt.dr,
                        cfmt.er,
                        cfmt.wi,
                        cfmt.di,
                        cfmt.ei,
                        OUT_DEC,
                    ))
                }
            }
            SEXPTYPE::STRSXP => {
                let quote_ch = if quote {
                    b'"' as std::os::raw::c_int
                } else {
                    0
                };
                encode_cstr(crate::mainutils::printutils::EncodeString(
                    crate::sexp::accessors::STRING_ELT(raw, i),
                    w,
                    quote_ch,
                    str_adj,
                ))
            }
            SEXPTYPE::RAWSXP => encode_cstr(crate::mainutils::printutils::EncodeRaw(
                crate::sexp::accessors::RAW_ELT(raw, i32i),
                std::ptr::null(),
            )),
            _ => String::new(),
        }
    }
}

/// Stock printVector: unnamed vector with "[i]" index labels, wrapping at
/// options("width"). Every element occupies the common field width.
pub(crate) unsafe fn print_vector_stock(x: Sexp, quote: bool, n_pr: R_xlen_t) -> String {
    // SAFETY: `x` is a rooted live SEXP and `n_pr` is bounded by its length;
    // all raw access remains read-only for the duration of this call.
    unsafe {
        let raw = x.clone().as_raw();
        let tp = x.typeof_();
        let (print_width, gap, _max) = vector_print_settings();
        let (mut w, d, e) = type_field_width(raw, tp, n_pr, quote);
        let mut out = String::new();
        let labwidth = crate::mainutils::printutils::IndexWidth_xlen(n_pr) + 2;
        let mut width = labwidth;
        out.push_str(&vector_index(1, labwidth as usize));
        if !matches!(tp, SEXPTYPE::STRSXP | SEXPTYPE::RAWSXP) {
            w += gap;
        }
        let is_str = matches!(tp, SEXPTYPE::STRSXP);
        let gap_prefixed = is_str || tp == SEXPTYPE::RAWSXP;
        let cfmt = if tp == SEXPTYPE::CPLXSXP {
            complex_fmt(raw, n_pr)
        } else {
            ComplexFmt {
                wr: 0,
                dr: 0,
                er: 0,
                wi: 0,
                di: 0,
                ei: 0,
            }
        };
        for i in 0..n_pr {
            if i > 0 {
                // stock wrap conditions: char adds the gap to the check and to
                // the accumulated width; raw prefixes the gap but counts only w.
                let wrap = if is_str {
                    width + w + gap > print_width
                } else {
                    width + w > print_width
                };
                if wrap {
                    out.push('\n');
                    out.push_str(&vector_index(i + 1, labwidth as usize));
                    width = labwidth;
                }
            }
            if gap_prefixed {
                out.push_str(&" ".repeat(gap as usize));
            }
            out.push_str(&encode_element_adj(
                raw,
                tp,
                i,
                w,
                d,
                e,
                quote,
                gap,
                cfmt,
                crate::mainutils::printutils::Rprt_adj::left,
            ));
            width += if is_str { w + gap } else { w };
        }
        out.push('\n');
        out
    }
}
/// Stock printNamedVector: names line(s) right-justified over the value
/// column(s), every column at the common width `w`, gap-separated.
pub(crate) unsafe fn print_named_vector_stock(
    x: Sexp,
    names: Sexp,
    quote: bool,
    n_pr: R_xlen_t,
) -> String {
    // SAFETY: `x` and `names` are rooted live SEXPs with compatible lengths;
    // element access is read-only and bounded by `n_pr`.
    unsafe {
        let raw = x.clone().as_raw();
        let names_raw = names.as_raw();
        let tp = x.typeof_();
        let (print_width, gap, _max) = vector_print_settings();
        let (mut w, d, e) = type_field_width(raw, tp, n_pr, quote);
        let mut wn = 0;
        let cfmt0 = if tp == SEXPTYPE::CPLXSXP {
            complex_fmt(raw, n_pr)
        } else {
            ComplexFmt {
                wr: 0,
                dr: 0,
                er: 0,
                wi: 0,
                di: 0,
                ei: 0,
            }
        };
        crate::mainutils::format::formatStringS(names_raw, n_pr, &mut wn, 0);
        if w < wn {
            w = wn;
        }
        let nperline = ((print_width / (w + gap)).max(1)) as R_xlen_t;
        let nlines = n_pr / nperline + R_xlen_t::from(n_pr % nperline != 0);
        let mut out = String::new();
        for i in 0..nlines {
            if i != 0 {
                out.push('\n');
            }
            for j in 0..nperline {
                let k = i * nperline + j;
                if k >= n_pr {
                    break;
                }
                out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeString(
                    crate::sexp::accessors::STRING_ELT(names_raw, k),
                    w,
                    0,
                    crate::mainutils::printutils::Rprt_adj::right,
                )));
                out.push_str(&" ".repeat(gap as usize));
            }
            out.push('\n');
            for j in 0..nperline {
                let k = i * nperline + j;
                if k >= n_pr {
                    break;
                }
                let i32k = k as std::os::raw::c_int;
                if matches!(tp, SEXPTYPE::CPLXSXP) {
                    if j != 0 {
                        out.push_str(&" ".repeat(gap as usize));
                    }
                    let c = crate::sexp::accessors::COMPLEX_ELT(raw, i32k);
                    if part_is_na(c.r) || part_is_na(c.i) {
                        out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                            crate::sexp::ffi::NA_REAL,
                            w,
                            0,
                            0,
                            OUT_DEC,
                        )));
                    } else {
                        out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                            c.r, cfmt0.wr, cfmt0.dr, cfmt0.er, OUT_DEC,
                        )));
                        if part_is_nan(c.i) {
                            out.push_str("+NaNi");
                        } else if c.i >= 0.0 {
                            out.push('+');
                            out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                                c.i, cfmt0.wi, cfmt0.di, cfmt0.ei, OUT_DEC,
                            )));
                            out.push('i');
                        } else {
                            out.push('-');
                            out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                                -c.i, cfmt0.wi, cfmt0.di, cfmt0.ei, OUT_DEC,
                            )));
                            out.push('i');
                        }
                    }
                } else if matches!(tp, SEXPTYPE::RAWSXP) {
                    // stock: "%*s%s%*s" with w-2, raw, gap
                    out.push_str(&" ".repeat((w - 2).max(0) as usize));
                    out.push_str(&encode_element_at(raw, tp, k, w, d, e, quote, gap, cfmt0));
                    out.push_str(&" ".repeat(gap as usize));
                } else {
                    let str_quote = if matches!(tp, SEXPTYPE::STRSXP) {
                        quote
                    } else {
                        false
                    };
                    out.push_str(&encode_element_at(
                        raw, tp, k, w, d, e, str_quote, gap, cfmt0,
                    ));
                    out.push_str(&" ".repeat(gap as usize));
                }
            }
        }
        out.push('\n');
        out
    }
}

/// Render an atomic vector exactly like stock print.default: named vectors go
/// through printNamedVector, others through printVector with index labels;
/// both honour options("max.print") truncation.
pub(crate) unsafe fn format_vector_stock(x: Sexp, quote: bool) -> String {
    unsafe { format_vector_stock_n(x, quote, None) }
}

pub(crate) unsafe fn format_vector_stock_n(
    x: Sexp,
    quote: bool,
    max_override: Option<i64>,
) -> String {
    if let Some(message) = result_admission_error(x.clone()) {
        return message;
    }
    unsafe {
        let n = x.clone().len();
        if n == 0 {
            return match x.typeof_() {
                SEXPTYPE::LGLSXP => "logical(0)".to_string(),
                SEXPTYPE::INTSXP => "integer(0)".to_string(),
                SEXPTYPE::REALSXP => "numeric(0)".to_string(),
                SEXPTYPE::CPLXSXP => "complex(0)".to_string(),
                SEXPTYPE::STRSXP => "character(0)".to_string(),
                SEXPTYPE::RAWSXP => "raw(0)".to_string(),
                _ => String::new(),
            };
        }
        let (_width, _gap, opt_max) = vector_print_settings();
        let max = max_override.unwrap_or(opt_max);
        let n_pr = if n <= (max + 1) as R_xlen_t {
            n
        } else {
            max as R_xlen_t
        };

        let mut out = match names_sexp(x.clone()) {
            Some(names) => print_named_vector_stock(x, names, quote, n_pr),
            None => print_vector_stock(x, quote, n_pr),
        };
        if n_pr < n {
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]\n",
                n - n_pr
            ));
        }
        // Callers own the final newline (print_value appends it; string contexts
        // embed the rendering without one).
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }
}

/// Names used by stock printNamedVector.
///
/// GNU PrintValueRec for a 1-d array uses **only** `dimnames[[1]]`.
/// A 1-d array with a `names` attribute but empty dimnames prints
/// without a header. Fall back to the names attribute for non-arrays.
fn names_sexp(x: Sexp<'_>) -> Option<Sexp<'_>> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 1 {
            let dimnames = crate::sexp::attrib_core::getAttrib(
                x.clone().as_raw(),
                crate::sexp::attrib_core::R_DimNamesSymbol(),
            );
            if !dimnames.is_null()
                && dimnames != crate::sexp::globals::R_NilValue()
                && TYPEOF(dimnames) == SEXPTYPE::VECSXP
                && XLENGTH(dimnames) >= 1
            {
                let row = VECTOR_ELT(dimnames, 0);
                if let Some(row) = Sexp::from_raw(row)
                    && row.clone().typeof_() == SEXPTYPE::STRSXP
                    && row.clone().len() == x.len()
                {
                    return Some(row);
                }
            }
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = Sexp::from_raw(names)?;
        if names.clone().typeof_() == SEXPTYPE::STRSXP && names.clone().len() == x.len() {
            Some(names)
        } else {
            None
        }
    }
}

/// Print an R object to the captured output (or stdout if not capturing).
///
/// This is the Rust implementation of R's Rf_PrintValue. For Android
/// embedding, use [`start_capture`] before evaluation and [`stop_capture`]
/// after to collect printed output as a string.
pub fn print_value(x: Sexp<'_>) {
    if let Some(message) = result_admission_error(x.clone()) {
        emit(&message);
        emit("\n");
        return;
    }
    // Objects of class "try-error" print per stock print.default: the
    // message string as a character vector plus class/condition attrs.
    // Condition objects print via print.condition.
    if has_class(x.clone(), "try-error") {
        unsafe { emit(&format!("{}\n", format_try_error(x))) };
        return;
    }
    if has_class(x.clone(), "condition") {
        unsafe { emit(&format!("{}\n", format_condition(x))) };
        return;
    }
    if has_class(x.clone(), "srcref") {
        unsafe {
            crate::mainutils::print::PrintValueEnv(
                x.clone().as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }
        return;
    }

    // GNU auto-print of S4 objects goes through PrintValueEnv -> show().
    if unsafe { crate::mainutils::objects::IS_S4_OBJECT(x.clone().as_raw()) } != 0 {
        unsafe {
            crate::mainutils::print::PrintValueEnv(
                x.clone().as_raw(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }
        return;
    }


    match x.clone().typeof_() {
        SEXPTYPE::SYMSXP | SEXPTYPE::LANGSXP | SEXPTYPE::CLOSXP => {
            let base = deparse_expression_one(x.clone().as_raw());
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP => {
            emit(&format!(
                "{}\n",
                format_with_printable_attributes(format_primitive(x.clone()), x)
            ));
        }


        SEXPTYPE::LISTSXP => {
            emit(&format!("{}\n", format_sexp_top_level(x)));
        }

        SEXPTYPE::NILSXP => {
            emit("NULL\n");
        }
        SEXPTYPE::INTSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.clone().len() == 0 {
                let empty = if has_names_attribute(x.clone()) {
                    "named integer(0)"
                } else {
                    "integer(0)"
                };
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes(empty.to_string(), x)
                ));
                return;
            }

            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_factor(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::REALSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.clone().len() == 0
                && !has_class(x.clone(), "difftime")
                && !has_class(x.clone(), "POSIXct")
                && !has_class(x.clone(), "Date")
            {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("numeric(0)".to_string(), x)
                ));
                return;
            }

            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if has_class(x.clone(), "difftime") {
                emit(&format!("{}\n", format_difftime_vector(x)));
                return;
            }
            if has_class(x.clone(), "POSIXct") {
                emit(&format!("{}\n", format_posixct_vector(x, true)));
                return;
            }
            if has_class(x.clone(), "Date") {
                emit(&format!("{}\n", format_date_vector(x)));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.clone().len() == 0 {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("logical(0)".to_string(), x)
                ));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }

        SEXPTYPE::CPLXSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.clone().len() == 0 {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("complex(0)".to_string(), x)
                ));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }

        SEXPTYPE::STRSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::RAWSXP => {
            if x.clone().len() == 0 {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("raw(0)".to_string(), x)
                ));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::VECSXP => {
            if has_class(x.clone(), "summary.warnings") {
                let text = unsafe {
                    crate::mainutils::essentials::format_summary_warnings(x.as_raw())
                };

                emit(&text);
                return;
            }

            if has_class(x.clone(), "POSIXlt") {
                emit(&format!("{}\n", format_posixlt_vector(x)));
                return;
            }
            emit(&format!("{}\n", format_sexp_top_level(x)));
        }

        SEXPTYPE::EXPRSXP => {
            emit(&format!("{}\n", format_expression_vector(x)));
        }
        SEXPTYPE::ENVSXP => {
            emit(&format!("{}\n", format_environment(x)));
        }
        tp => {
            let type_name = match tp {
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::SYMSXP => "symbol",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                SEXPTYPE::CHARSXP => "charsxp",
                SEXPTYPE::PROMSXP => "promise",
                SEXPTYPE::DOTSXP => "...",
                SEXPTYPE::SPECIALSXP => "special",
                SEXPTYPE::BUILTINSXP => "builtin",
                SEXPTYPE::EXTPTRSXP => "externalptr",
                SEXPTYPE::WEAKREFSXP => "weakref",
                SEXPTYPE::BCODESXP => "bytecode",
                SEXPTYPE::OBJSXP => "object",
                _ => "unknown",
            };
            let output = format!("[{}; length={}]", type_name, x.len());
            emit(&output);
            emit("\n");
        }
    }
}

fn emit(msg: &str) {
    if is_capturing() {
        capture_stdout(msg);
    } else {
        print!("{}", msg);
    }
}

fn result_admission_error(x: Sexp<'_>) -> Option<String> {
    let limit = super::instance::with_current_instance(|inst| {
        // SAFETY: current instance is active; this short immutable borrow ends
        // before traversing the rooted result, and performs no R allocation.
        unsafe { (*inst).output_capture.borrow().max_bytes }
    })
    .flatten()?;
    crate::android::result_budget::admit(x, limit)
        .err()
        .map(|message| format!("[{message}]"))
}

pub fn format_sexp_direct(x: Sexp<'_>) -> String {
    if let Some(message) = result_admission_error(x.clone()) {
        return message;
    }

    if has_class(x.clone(), "try-error") {
        return unsafe { format_try_error(x) };
    }
    if has_class(x.clone(), "condition") {
        return unsafe { format_condition(x) };
    }
    match x.clone().typeof_() {
        SEXPTYPE::NILSXP => "NULL".to_string(),
        SEXPTYPE::INTSXP => {
            if x.clone().len() == 0 {
                let empty = if has_names_attribute(x.clone()) {
                    "named integer(0)"
                } else {
                    "integer(0)"
                };
                return format_with_printable_attributes(empty.to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                return output;
            }
            if let Some(output) = format_table(x.clone()) {
                return output;
            }
            if let Some(output) = format_factor(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::REALSXP => {
            if has_class(x.clone(), "difftime") {
                return format_difftime_vector(x);
            }
            if has_class(x.clone(), "POSIXct") {
                return format_posixct_vector(x, true);
            }
            if has_class(x.clone(), "Date") {
                return format_date_vector(x);
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("numeric(0)".to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                return output;
            }
            if let Some(output) = format_table(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("logical(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }

        SEXPTYPE::CPLXSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("complex(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }

        SEXPTYPE::STRSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                return output;
            }
            let quote = print_quote_flag()
                && !has_class(x.clone(), "noquote")
                && !has_class(x.clone(), "table");
            let base = unsafe { format_vector_stock(x.clone(), quote) };
            format_with_printable_attributes(base, x)

        }


        SEXPTYPE::RAWSXP => {
            if x.clone().len() == 0 {
                return format_with_printable_attributes("raw(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::VECSXP => {
            if has_class(x.clone(), "summary.warnings") {
                return unsafe {
                    crate::mainutils::essentials::format_summary_warnings(x.as_raw())
                        .trim_end_matches('\n')
                        .to_string()
                };
            }
            if has_class(x.clone(), "POSIXlt") {
                return format_posixlt_vector(x);
            }
            if let Some(output) = format_data_frame(x.clone()) {
                return output;
            }
            format_list(x)
        }


        SEXPTYPE::EXPRSXP => format_expression_vector(x),
        SEXPTYPE::SYMSXP | SEXPTYPE::LANGSXP | SEXPTYPE::CLOSXP => {
            let base = deparse_expression_one(x.clone().as_raw());
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::SPECIALSXP | SEXPTYPE::BUILTINSXP => {
            format_with_printable_attributes(format_primitive(x.clone()), x)
        }

        SEXPTYPE::LISTSXP => format_pairlist(x),
        SEXPTYPE::ENVSXP => format_environment(x),
        tp => {
            let type_name = match tp {
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::CHARSXP => "charsxp",
                _ => "unknown",
            };
            format!("[{}; length={}]", type_name, x.len())
        }
    }
}

/// First class string of an object, for print.condition-style rendering.
unsafe fn first_class_string(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let klass = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let klass = Sexp::from_raw(klass)?;
        if klass.clone().typeof_() != SEXPTYPE::STRSXP || klass.clone().len() == 0 {
            return None;
        }
        let s = crate::sexp::accessors::STRING_ELT(klass.as_raw(), 0);
        if s.is_null() {
            return None;
        }
        let chars = crate::sexp::accessors::CHAR(s);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

/// Stock print.condition: `<class: msg>` or `<class in <deparsed call>: msg>`.
/// Conditions are `list(message, call, ...)` per R_makeErrorCondition.
unsafe fn format_condition(x: Sexp<'_>) -> String {
    // SAFETY: callers provide a rooted live condition object. Each optional
    // list/string field is tag-checked before raw element access.
    unsafe {
        let raw = x.clone().as_raw();
        let class = first_class_string(x.clone()).unwrap_or_else(|| "condition".to_string());
        let mut message = String::new();
        let mut call_text = String::new();
        if x.clone().typeof_() == SEXPTYPE::VECSXP && x.clone().len() >= 1 {
            let msg = crate::sexp::accessors::VECTOR_ELT(raw, 0);
            if !msg.is_null() && msg != crate::sexp::globals::R_NilValue() {
                if let Some(sexp) = Sexp::from_raw(msg) {
                    if sexp.clone().typeof_() == SEXPTYPE::STRSXP && sexp.clone().len() >= 1 {
                        let elt = crate::sexp::accessors::STRING_ELT(sexp.as_raw(), 0);
                        if !elt.is_null() {
                            let chars = crate::sexp::accessors::CHAR(elt);
                            if !chars.is_null() {
                                if let Ok(s) = std::ffi::CStr::from_ptr(chars).to_str() {
                                    message = s.to_string();
                                }
                            }
                        }
                    }
                }
            }
            if x.len() >= 2 {
                let call = crate::sexp::accessors::VECTOR_ELT(raw, 1);
                if !call.is_null() && call != crate::sexp::globals::R_NilValue() {
                    let dcall = crate::mainutils::deparse::deparse1s(call);
                    if !dcall.is_null() && dcall != crate::sexp::globals::R_NilValue() {
                        let elt = crate::sexp::accessors::STRING_ELT(dcall, 0);
                        if !elt.is_null() {
                            let chars = crate::sexp::accessors::CHAR(elt);
                            if !chars.is_null() {
                                if let Ok(s) = std::ffi::CStr::from_ptr(chars).to_str() {
                                    call_text = format!(" in {s}");
                                }
                            }
                        }
                    }
                }
            }
        }
        format!("<{class}{call_text}: {message}>")
    }
}

/// Stock print.default on a try-error object: the message string rendered as
/// a character vector, then the class and condition attributes.
unsafe fn format_try_error(x: Sexp<'_>) -> String {
    // SAFETY: callers provide a rooted live try-error object; attribute and
    // vector reads remain within the lifetime carried by `x`.
    unsafe {
        let mut out = if x.clone().typeof_() == SEXPTYPE::STRSXP {
            format_vector_stock(x.clone(), true)
        } else {
            format!("{}\n", format_sexp_direct(x.clone()))
        };
        out.push('\n');
        unsafe {
            let klass = crate::sexp::attrib_core::getAttrib(
                x.clone().as_raw(),
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            let has_condition = {
                let cond_sym = crate::sexp::symbol::Rf_install(
                    b"condition\0".as_ptr() as *const std::os::raw::c_char
                );
                let cond = crate::sexp::attrib_core::getAttrib(x.clone().as_raw(), cond_sym);
                !cond.is_null() && cond != R_NilValue()
            };
            if let Some(klass) = Sexp::from_raw(klass) {
                if klass.clone().typeof_() == SEXPTYPE::STRSXP {
                    out.push_str("attr(,\"class\")\n");
                    out.push_str(&format_vector_stock(klass, true));
                    // print_value owns the final newline; add the separator
                    // between attribute sections here.
                    if has_condition {
                        out.push('\n');
                    }
                }
            }
            let cond_sym = crate::sexp::symbol::Rf_install(
                b"condition\0".as_ptr() as *const std::os::raw::c_char
            );
            let cond = crate::sexp::attrib_core::getAttrib(x.as_raw(), cond_sym);
            if let Some(cond) = Sexp::from_raw(cond) {
                if cond.clone().typeof_() != SEXPTYPE::NILSXP {
                    out.push_str("attr(,\"condition\")\n");
                    out.push_str(&format_condition(cond));
                }
            }
        }
        out
    }
}

pub(crate) fn format_environment_public(x: Sexp<'_>) -> String {
    format_environment(x)
}
fn format_environment(x: Sexp<'_>) -> String {
    let raw = x.as_raw();
    let name = unsafe {
        if raw == crate::sexp::globals::R_GlobalEnv() {
            "R_GlobalEnv".to_string()
        } else if raw == crate::sexp::globals::R_BaseEnv() {
            "base".to_string()
        } else if raw == crate::sexp::globals::R_EmptyEnv() {
            "R_EmptyEnv".to_string()
        } else {
            format!("{raw:p}")
        }
    };
    format!("<environment: {name}>")
}

/// Print an R object's structure (like str()).
pub fn print_structure(x: Sexp<'_>, indent: usize) {
    let prefix = "  ".repeat(indent);

    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => {
            let vals: Vec<_> = x.clone().iter_integer().take(10).collect();
            let suffix = if x.clone().len() > 10 { ", ..." } else { "" };
            let output = format!("{}int [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::REALSXP => {
            let vals: Vec<_> = x.clone().iter_real().take(10).collect();
            let suffix = if x.clone().len() > 10 { ", ..." } else { "" };
            let output = format!("{}double [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::STRSXP => {
            let output = format!("{}character [{}]", prefix, x.len());
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::VECSXP => {
            let output = format!("{}list [{}]", prefix, x.clone().len());
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
            for (i, elem) in x.iter_vector().take(5).enumerate() {
                print_structure(elem, indent + 1);
            }
        }
        _ => {
            print_value(x);
        }
    }
}

/// FFI function: Rf_PrintValue
pub(crate) unsafe fn Rf_PrintValue(x: SEXP) {
    if let Some(s) = Sexp::from_raw(x) {
        print_value(s);
    }
}

/// FFI function: Rf_PrintValueEnv (print with environment context)
pub(crate) unsafe fn Rf_PrintValueEnv(x: SEXP, _env: SEXP) {
    if let Some(s) = Sexp::from_raw(x) {
        print_value(s);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)] // translated tests exercise the Sexp compat setters
mod tests {
    use super::*;
    use crate::sexp::instance::RInstance;
    use crate::sexp::session::RSession;

    #[test]
    fn test_capture_lifecycle() {
        let _session = RSession::new();
        assert!(!is_capturing());
        start_capture();
        assert!(is_capturing());
        capture_stdout("hello ");
        capture_stdout("world\n");
        capture_stderr("warning!\n");
        let output = stop_capture();
        assert_eq!(output.stdout, "hello world\n");
        assert_eq!(output.stderr, "warning!\n");
        assert!(!is_capturing());
    }

    #[test]
    fn test_capture_empty() {
        let _session = RSession::new();
        start_capture();
        let output = stop_capture();
        assert_eq!(output.stdout, "");
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn test_nested_capture() {
        let _session = RSession::new();
        start_capture();
        capture_stdout("outer ");
        capture_stderr("outer err ");

        start_capture();
        capture_stdout("inner ");
        capture_stderr("inner err ");
        let inner = stop_capture();
        assert_eq!(inner.stdout, "inner ");
        assert_eq!(inner.stderr, "inner err ");

        assert!(is_capturing());
        capture_stdout("resumed");
        capture_stderr("resumed err");

        let outer = stop_capture();
        assert_eq!(outer.stdout, "outer resumed");
        assert_eq!(outer.stderr, "outer err resumed err");
        assert!(!is_capturing());
    }

    #[test]
    fn selective_capture_crosses_layers_and_charges_receiving_budget() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(4));
        state.start();
        state.capture_stdout("ab");
        state.start_with_options(false, true, false);
        state.start_with_options(false, true, false);
        state.capture_stdout("cde");
        state.capture_stderr("x");
        let inner = state.stop();
        assert_eq!(inner.stderr, "x");
        assert!(!inner.truncated);
        assert_eq!(state.stop().stderr, "");
        let outer = state.stop();
        assert_eq!(outer.stdout, "abcd");
        assert!(outer.truncated);
    }

    #[test]
    fn nested_split_captures_tee_once_and_keep_separate_budgets() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(4));
        state.start();
        state.capture_stdout("ab");
        state.start_with_options(true, false, true);
        state.start_with_options(true, false, true);
        state.capture_stdout("cde");
        for _ in 0..2 {
            let inner = state.stop();
            assert_eq!(inner.stdout, "cde");
            assert!(!inner.truncated);
        }
        let outer = state.stop();
        assert_eq!(outer.stdout, "abcd");
        assert!(outer.truncated);
    }

    #[test]
    fn test_bounded_capture_respects_utf8_boundaries() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(5));
        state.start();
        state.capture_stdout("ééé");
        let output = state.stop();

        assert_eq!(output.stdout, "éé");
        assert_eq!(output.stdout.len(), 4);
        assert!(output.truncated);
    }

    #[test]
    fn test_bounded_capture_exact_limit_and_empty_message_are_not_truncated() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(3));
        state.start();
        state.capture_stdout("abc");
        state.capture_stderr("");
        let output = state.stop();

        assert_eq!(output.stdout, "abc");
        assert!(!output.truncated);
    }

    #[test]
    fn test_nested_bounded_capture_restores_outer_state() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(4));
        state.start();
        state.capture_stdout("ab");

        state.start();
        state.capture_stdout("12345");
        let inner = state.stop();
        assert_eq!(inner.stdout, "1234");
        assert!(inner.truncated);

        state.capture_stdout("cd");
        let outer = state.stop();
        assert_eq!(outer.stdout, "abcd");
        assert!(!outer.truncated);
    }

    #[test]
    fn test_bounded_capture_budget_is_shared_by_stdout_and_stderr() {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(5));
        state.start();
        state.capture_stdout("abc");
        state.capture_stderr("def");
        let output = state.stop();

        assert_eq!(output.stdout, "abc");
        assert_eq!(output.stderr, "de");
        assert!(output.truncated);
        assert_eq!(output.stdout.len() + output.stderr.len(), 5);
    }

    #[test]
    fn test_capture_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        start_capture_in(&mut left);
        capture_stdout_in(&mut left, "left");
        capture_stderr_in(&mut left, "left err");
        assert!(is_capturing_in(&mut left));
        assert!(!is_capturing_in(&mut right));

        start_capture_in(&mut right);
        capture_stdout_in(&mut right, "right");
        let right_output = stop_capture_in(&mut right);
        assert_eq!(right_output.stdout, "right");
        assert_eq!(right_output.stderr, "");

        let left_output = stop_capture_in(&mut left);
        assert_eq!(left_output.stdout, "left");
        assert_eq!(left_output.stderr, "left err");
    }

    #[test]
    fn test_print_logical_vector() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, 3);
                let sexp = Sexp::from_raw(ptr).expect("logical vector allocation failed");
                assert!(sexp.clone().set_logical_elt(0, 0));
                assert!(sexp.clone().set_logical_elt(1, 1));
                assert!(
                    sexp.clone()
                        .set_logical_elt(2, crate::sexp::ffi::NA_LOGICAL)
                );

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1] FALSE  TRUE    NA\n");
            })
            .unwrap();
    }

    #[test]
    fn test_print_named_list_keeps_final_stock_separator_line() {
        let mut session = RSession::new();
        let (result, output, _) =
            session.eval_code_with_output_capture("print(list(class = 'ts'))");

        assert!(result.is_ok(), "named list should evaluate: {result:?}");
        assert_eq!(output.stdout, "$class\n[1] \"ts\"\n\n");
    }

    #[test]
    fn test_string_output_uses_safe_charsxp_access_and_preserves_na() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 2);
                let sexp = Sexp::from_raw(ptr).expect("string vector allocation failed");
                let value = Sexp::from_raw(arena.alloc_charsxp(b"a")).expect("CHARSXP");
                let missing = Sexp::from_raw(unsafe { crate::sexp::globals::R_NaString() })
                    .expect("NA_STRING");
                sexp.clone()
                    .try_set_string_elt(0, value)
                    .expect("set string");
                sexp.clone()
                    .try_set_string_elt(1, missing)
                    .expect("set string");

                assert_eq!(format_sexp_direct(sexp.clone()), "[1] \"a\" NA ");

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                // Stock R pads the final NA to the field width even in last
                // position (verified against R: `print(c("a", NA))` emits
                // "[1] \"a\" NA \n" with the trailing space).
                assert_eq!(output.stdout, "[1] \"a\" NA \n");
            })
            .unwrap();
    }

    #[test]
    fn test_atomic_element_formatting_reports_access_errors() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let real = Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 1))
                    .expect("real vector allocation failed");

                assert!(
                    format_integer_element(real.clone(), 0).contains("expected integer vector")
                );
                assert!(format_real_element(real, 2).contains("outside vector length"));
            })
            .unwrap();
    }

    #[test]
    fn test_format_atomic_na_values() {
        assert_eq!(format_integer_value(crate::sexp::ffi::NA_INTEGER), "NA");
        assert_eq!(format_real_value(crate::sexp::ffi::NA_REAL), "NA");
        assert_eq!(format_real_value(f64::NAN), "NaN");
        assert_eq!(format_real_value(f64::INFINITY), "Inf");
        assert_eq!(format_real_value(f64::NEG_INFINITY), "-Inf");
    }

    #[test]
    fn test_numeric_vector_alignment_matches_r_simple_output() {
        let vals = vec!["2".to_string(), "NA".to_string(), "4".to_string()];
        assert_eq!(format_aligned_values(vals), " 2 NA  4");
    }

    #[test]
    fn test_real_vector_alignment_keeps_decimal_column() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
                let sexp = Sexp::from_raw(ptr).expect("real vector allocation failed");
                sexp.clone().try_set_real_elt(0, 200.0).expect("set real");
                sexp.clone().try_set_real_elt(1, 80200.0).expect("set real");
                sexp.clone().try_set_real_elt(2, 100.5).expect("set real");

                assert_eq!(
                    format_sexp_direct(sexp.clone()),
                    "[1]   200.0 80200.0   100.5"
                );

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1]   200.0 80200.0   100.5\n");
            })
            .unwrap();
    }
}
