use super::*;

// ---------------------------------------------------------------------------
// Complete I/O
// ---------------------------------------------------------------------------

/// R-like `cat_args(...)` — cat with better formatting.
pub unsafe fn do_cat_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_cat(_call, _op, args, _rho) }
}

/// R-like `message_args(...)` — message with domain.
pub unsafe fn do_message_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `packageStartupMessage(...)` — startup message.
pub unsafe fn do_package_startup_message(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        let message = if output.ends_with('\n') {
            output
        } else {
            format!("{output}\n")
        };
        let condition = super::conditions::simple_condition(
            &message,
            &[
                "packageStartupMessage",
                "simpleMessage",
                "message",
                "condition",
            ],
        );
        let _c = protect(condition);
        let wrapped = Rf_cons(condition, R_NilValue());
        let _w = protect(wrapped);
        super::conditions::do_message(call, op, wrapped, rho)
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — capture.output, withVisible, invisible, suppress*,
// ---------------------------------------------------------------------------

/// Filename destinations and connections opened by capture.output are owned. Close them
/// against their originating session, including when evaluation unwinds.
struct CaptureFileGuard {
    instance: *mut crate::sexp::instance::RInstance,
    connection: Option<usize>,
}

/// Restores `sink(type="message")` after `capture.output` evaluates its
/// expressions, including when that evaluation unwinds.
struct MessageSinkGuard {
    instance: *mut crate::sexp::instance::RInstance,
    previous: i32,
}

impl MessageSinkGuard {
    fn push(index: i32) -> Self {
        let instance = crate::sexp::instance::with_required_current_instance(|instance| instance);
        let previous = unsafe { (*instance).connections_state.sink.error_con };
        unsafe {
            (*instance).connections_state.sink.error_con = index;
        }
        Self { instance, previous }
    }
}

impl Drop for MessageSinkGuard {
    fn drop(&mut self) {
        unsafe {
            (*self.instance).connections_state.sink.error_con = self.previous;
        }
    }
}
impl CaptureFileGuard {
    fn close(&mut self) {
        if let Some(index) = self.connection.take() {
            unsafe {
                if let Some(mut connection) = (&mut (*self.instance).connections_state.table)
                    .get_mut(index)
                    .and_then(Option::take)
                {
                    crate::mainutils::connections::close_connection_inner(&mut connection);
                }
            }
        }
    }
}
impl Drop for CaptureFileGuard {
    fn drop(&mut self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.close()));
    }
}

/// R's `capture.output(expr)` — capture printed stdout as a character vector.
pub unsafe fn do_capture_output(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let control_name = |cell: SEXP| -> Option<String> {
            let tag = TAG(cell);
            if tag.is_null() || tag == R_NilValue() {
                return None;
            }
            let name = CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
            matches!(name.as_ref(), "file" | "append" | "type" | "split").then(|| name.into_owned())
        };
        let mut capture_type = "output".to_string();
        let mut split = false;
        let mut file_value = R_NilValue();
        let mut _file_root = None;
        let mut append_file = false;
        let mut control = args;
        let mut seen = std::collections::HashSet::new();
        while control != R_NilValue() && !control.is_null() {
            if let Some(name) = control_name(control) {
                if !seen.insert(name.clone()) {
                    base_error("formal argument matched by multiple actual arguments");
                }
                let value = crate::eval::eval::Rf_eval(CAR(control), rho);
                match name.as_str() {
                    "file" if value != R_NilValue() => {
                        file_value = value;
                        _file_root = Some(protect(value));
                    }
                    "type" => {
                        if TYPEOF(value) != SEXPTYPE::STRSXP || LENGTH(value) != 1 {
                            base_error("invalid 'type' argument");
                        }
                        let kind = CStr::from_ptr(CHAR(STRING_ELT(value, 0))).to_string_lossy();
                        capture_type = ["output", "message"]
                            .into_iter()
                            .find(|candidate| {
                                !kind.is_empty() && candidate.starts_with(kind.as_ref())
                            })
                            .unwrap_or_else(|| base_error("invalid 'type' argument"))
                            .into();
                    }
                    "split" | "append" => {
                        let enabled = crate::main::coerce::asLogical(value);
                        if enabled == NA_LOGICAL {
                            base_error(format!("invalid '{name}' argument"));
                        }
                        if name == "split" && enabled != FALSE {
                            split = true;
                        }
                        if name == "append" {
                            append_file = enabled != FALSE;
                        }
                    }
                    _ => {}
                }
            }
            control = CDR(control);
        }
        if capture_type == "message" && split {
            base_error("cannot split the message connection");
        }
        let mut file_guard = CaptureFileGuard {
            instance: crate::sexp::instance::with_required_current_instance(|instance| instance),
            connection: None,
        };
        let destination = if file_value == R_NilValue() {
            None
        } else {
            let connection = if inherits_class(file_value, "connection") {
                let index = connection_index(file_value);
                let slot = crate::mainutils::connections::checked_connection_index(index);
                let is_open = {
                    let table = crate::mainutils::connections::connection_table();
                    table[slot].as_ref().expect("validated connection").isopen
                };
                if !is_open {
                    let mode = Rf_mkString(if append_file {
                        c"a".as_ptr()
                    } else {
                        c"w".as_ptr()
                    });
                    let _mode = protect(mode);
                    let tail = Rf_cons(mode, R_NilValue());
                    let _tail = protect(tail);
                    let open_args = Rf_cons(file_value, tail);
                    let _open_args = protect(open_args);
                    crate::mainutils::connections::do_open(_call, _op, open_args, rho);
                    file_guard.connection = Some(slot);
                }
                index
            } else if TYPEOF(file_value) == SEXPTYPE::STRSXP && LENGTH(file_value) == 1 {
                let mode = Rf_mkString(if append_file {
                    c"a".as_ptr()
                } else {
                    c"w".as_ptr()
                });
                let _mode = protect(mode);
                let tail = Rf_cons(mode, R_NilValue());
                let _tail = protect(tail);
                let file_args = Rf_cons(file_value, tail);
                let _file_args = protect(file_args);
                let connection = crate::mainutils::connections::do_file(_call, _op, file_args, rho);
                let index = connection_index(connection);
                file_guard.connection = Some(index as usize);
                index
            } else {
                base_error("'file' must be NULL, a connection or a character string");
            };
            // Validate before evaluating the expression, even if it prints nothing.
            crate::mainutils::connections::connection_write_bytes(connection, b"");
            Some(connection)
        };
        let mut capture = crate::sexp::output::OutputCaptureGuard::start_with_options(
            capture_type == "output",
            capture_type == "message",
            split,
        );
        if let Some(index) = destination {
            capture.set_connection(index);
        }
        // GNU capture.output(type="message", file=) is sink(file, type="message").
        // message() follows that connection rather than the output capture frame.
        let message_sink = if capture_type == "message" {
            destination.map(MessageSinkGuard::push)
        } else {
            None
        };
        let mut argument = args;
        while argument != R_NilValue() && !argument.is_null() {
            if control_name(argument).is_some() {
                argument = CDR(argument);
                continue;
            }
            let expr = CAR(argument);
            if expr == crate::sexp::symbol::R_DotsSymbol() {
                let h = crate::sexp::envir::R_findVar(expr, rho);
                if TYPEOF(h) == SEXPTYPE::DOTSXP || h == R_NilValue() {
                    let mut dh = h;
                    while !dh.is_null() && dh != R_NilValue() {
                        let value = crate::eval::eval::Rf_eval(CAR(dh), rho);
                        let _value_root = protect(value);
                        if crate::sexp::globals::R_Visible() != FALSE {
                            let print_args = Rf_cons(value, R_NilValue());
                            let _print_args_root = protect(print_args);
                            crate::mainutils::essentials_basic::do_print(
                                _call, _op, print_args, rho,
                            );
                        }
                        dh = CDR(dh);
                    }
                    argument = CDR(argument);
                    continue;
                }
            }
            let value = crate::eval::eval::Rf_eval(expr, rho);
            let _value_root = protect(value);
            if crate::sexp::globals::R_Visible() != FALSE {
                let print_args = Rf_cons(value, R_NilValue());
                let _print_args_root = protect(print_args);
                crate::mainutils::essentials_basic::do_print(_call, _op, print_args, rho);
            }
            argument = CDR(argument);
        }
        drop(message_sink);
        let captured = capture.finish();

        let captured_stream = if capture_type == "message" {
            captured.stderr
        } else {
            captured.stdout
        };
        if destination.is_some() {
            file_guard.close();
            crate::sexp::globals::set_R_Visible(FALSE);
            return R_NilValue();
        }
        let stdout = captured_stream
            .strip_suffix('\n')
            .unwrap_or(&captured_stream);
        let lines: Vec<&str> = if captured_stream.is_empty() {
            Vec::new()
        } else {
            stdout.split('\n').collect()
        };

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, line) in lines.iter().enumerate() {
            let cstr = CString::new(*line).unwrap_or_default();
            let charsxp = Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

/// R's `withVisible(x)` — returns a list with $value and $visible.
pub unsafe fn do_with_visible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let visible = crate::sexp::globals::R_Visible();
        // Return a VECSXP (list) with two elements: value, visible
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 0, x);
        let vis_vec = Rf_ScalarLogical(visible);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 1, vis_vec);
        // Set names
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !names.is_null() {
            let _n_p = crate::sexp::protect::protect(names);
            let v_str = c"value";
            let vi_str = c"visible";
            let v_char = crate::sexp::constructors::Rf_mkChar(v_str.as_ptr());
            let vi_char = crate::sexp::constructors::Rf_mkChar(vi_str.as_ptr());
            if !v_char.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(names, 0 as R_xlen_t, v_char);
            }
            if !vi_char.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(names, 1 as R_xlen_t, vi_char);
            }
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"names".as_ptr()), names);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

/// R's `invisible(x)` — return x, setting visibility to FALSE.
/// `invisible()` with no argument is NULL (inspect.c do_invisible + missing formal).
pub unsafe fn do_invisible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(args) != SEXPTYPE::LISTSXP && TYPEOF(args) != SEXPTYPE::LANGSXP {
            return R_NilValue();
        }

        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || x == crate::sexp::globals::R_MissingArg() {
            R_NilValue()
        } else {
            x
        }
    }
}

/// R's `suppressWarnings(expr)` — evaluate expr with warnings suppressed.
///
/// Two mechanisms: stderr produced inside is dropped (covers the R-level
/// `warning()` builtin), and the errors-machinery suppress depth is raised so
/// C-level warnings (`Rf_warning` → collection) are dropped before collection,
/// matching upstream's muffle restart.
pub unsafe fn do_suppress_warnings(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let mut classes: Option<Vec<String>> = None;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                let name = if pname.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pname))
                        .to_string_lossy()
                        .into_owned()
                };
                if name == "classes" {
                    let value = crate::eval::eval::Rf_eval(CAR(cell), rho);
                    if !value.is_null()
                        && value != R_NilValue()
                        && TYPEOF(value) == SEXPTYPE::STRSXP
                    {
                        let mut names = Vec::new();
                        for i in 0..XLENGTH(value) {
                            let elt = crate::sexp::accessors::STRING_ELT(value, i);
                            if !elt.is_null() {
                                names.push(
                                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                                        .to_string_lossy()
                                        .into_owned(),
                                );
                            }
                        }
                        classes = Some(names);
                    }
                }
            }
            cell = CDR(cell);
        }
        struct DepthGuard;
        impl Drop for DepthGuard {
            fn drop(&mut self) {
                crate::mainutils::errors::pop_suppress_warning_classes();
                crate::mainutils::errors::exit_suppress_warnings();
            }
        }
        crate::mainutils::errors::push_suppress_warning_classes(classes);
        crate::mainutils::errors::enter_suppress_warnings();
        let _depth_guard = DepthGuard;
        crate::sexp::output::start_capture();
        let result = crate::eval::eval::Rf_eval(expr, rho);
        let captured = crate::sexp::output::stop_capture();
        if !captured.stdout.is_empty() {
            crate::sexp::output::capture_stdout(&captured.stdout);
        }
        result
    }
}

/// R's `suppressMessages(expr)` — evaluate expr with captured diagnostics suppressed.
pub unsafe fn do_suppress_messages(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let mut classes: Option<Vec<String>> = None;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                let name = if pname.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pname))
                        .to_string_lossy()
                        .into_owned()
                };
                if name == "classes" {
                    let value = crate::eval::eval::Rf_eval(CAR(cell), rho);
                    if !value.is_null()
                        && value != R_NilValue()
                        && TYPEOF(value) == SEXPTYPE::STRSXP
                    {
                        let mut names = Vec::new();
                        for i in 0..XLENGTH(value) {
                            let elt = crate::sexp::accessors::STRING_ELT(value, i);
                            if !elt.is_null() {
                                names.push(
                                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(elt))
                                        .to_string_lossy()
                                        .into_owned(),
                                );
                            }
                        }
                        classes = Some(names);
                    }
                }
            }
            cell = CDR(cell);
        }
        struct DepthGuard;
        impl Drop for DepthGuard {
            fn drop(&mut self) {
                crate::mainutils::errors::pop_suppress_message_classes();
                crate::mainutils::errors::exit_suppress_messages();
            }
        }
        crate::mainutils::errors::push_suppress_message_classes(classes);
        crate::mainutils::errors::enter_suppress_messages();
        let _depth_guard = DepthGuard;
        crate::eval::eval::Rf_eval(expr, rho)
    }
}

/// R's `force(x)` — force evaluation of a promise.
pub unsafe fn do_force(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // If x is a PROMSXP, force it
        if TYPEOF(x) == SEXPTYPE::PROMSXP {
            crate::sexp::envir::forcePromise(x)
        } else {
            x
        }
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — enhanced cat, message, warning
// ---------------------------------------------------------------------------

/// R's enhanced `cat(..., file, sep, fill, labels, append)` — simplified.
pub unsafe fn do_cat_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: delegates to existing do_cat
        do_cat(_call, _op, args, _rho)
    }
}

/// R's enhanced `message(..., domain, appendLF)` — simplified.
pub unsafe fn do_message_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        R_NilValue()
    }
}

/// R's enhanced `warning(..., call., immediate., noBreaks., domain.)` — simplified.
pub unsafe fn do_warning_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut output =
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"]);
        if output.is_empty() {
            output = "warning".to_string();
        }
        eprintln!("Warning: {}", output);
        R_NilValue()
    }
}
