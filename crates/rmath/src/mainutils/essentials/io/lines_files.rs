use super::*;

// ---------------------------------------------------------------------------
// I/O builtins: cat() to file, writeLines(), file.exists()
// ---------------------------------------------------------------------------

/// Resolve a file path from a table-reading builtin against the package
/// directory currently being sourced, when one is active.
///
/// Upstream evaluates install-time package code (crayon's
/// `read.table("tools/ansi-palettes.txt", ...)`) with the package root as
/// the working directory, baking the result into the lazy-load database.
/// This port sources package R files at load time instead, so a relative
/// path from that code is re-rooted at the package directory. Absolute
/// paths and ordinary session reads (no package being sourced) pass
/// through unchanged.
pub(crate) fn resolve_package_relative_path(file_path: String) -> String {
    let given = std::path::Path::new(&file_path);
    if given.is_absolute() {
        return file_path;
    }
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .loading_package_dir
            .as_ref()
            .map(|dir| dir.join(given))
    })
    .map(|joined| joined.to_string_lossy().into_owned())
    .unwrap_or(file_path)
}

/// R's `writeLines(text, con = stdout(), sep = "\n", useBytes = FALSE)`.
pub unsafe fn do_writeLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        if text.is_null() || text == R_NilValue() {
            return R_NilValue();
        }

        let mut con = R_NilValue();
        let mut sep = "\n".to_string();
        let mut positional = 0;
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match tag_name(current).as_deref() {
                Some("con") => con = arg,
                Some("sep") => sep = elt_to_string(arg, 0),
                Some("useBytes") => {}
                _ => {
                    match positional {
                        0 => con = arg,
                        1 => sep = elt_to_string(arg, 0),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let path = if con.is_null() || con == R_NilValue() {
            "/dev/stdout".to_string()
        } else if TYPEOF(con) == SEXPTYPE::INTSXP {
            let sep_sxp = Rf_mkString(CString::new(sep).unwrap_or_default().as_ptr());
            let normalized = Rf_cons(
                text,
                Rf_cons(
                    con,
                    Rf_cons(sep_sxp, Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue())),
                ),
            );
            return crate::mainutils::connections::do_writeLines(_call, _op, normalized, _rho);
        } else {
            elt_to_string(con, 0)
        };

        let n = if TYPEOF(text) == SEXPTYPE::STRSXP {
            XLENGTH(text)
        } else {
            1
        };
        if path == "/dev/stdout" {
            let mut output = String::new();
            for i in 0..n {
                output.push_str(&elt_to_string(text, i));
                output.push_str(&sep);
            }
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else {
            let mut bytes = Vec::new();
            for i in 0..n {
                bytes.extend_from_slice(elt_to_string(text, i).as_bytes());
                bytes.extend_from_slice(sep.as_bytes());
            }
            if let Err(error) = crate::mainutils::browser_files::write_text_or_host(&path, &bytes) {
                base_error(error.to_string());
            }
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `readLines(con)` — read lines from file.
pub unsafe fn do_readLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if con.is_null() {
            return R_NilValue();
        }
        let path = elt_to_string(con, 0);

        let lines = crate::mainutils::browser_files::read_text_or_host(&path)
            .unwrap_or_else(|error| base_error(error.to_string()));
        let line_vec: Vec<&str> = lines.lines().collect();
        let n = line_vec.len();

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, line) in line_vec.iter().enumerate() {
            let cstr = CString::new(*line).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        result
    }
}

/// R's `file.exists(...)` — check if files exist.
pub unsafe fn do_file_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).exists()
                || crate::library::builtin_files::path_exists(&path)
            {
                TRUE
            } else {
                FALSE
            };
        }
        result
    }
}

/// R's `list.files(path)` — list files in directory.
pub unsafe fn do_list_files(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let path_arg = CAR(args);
        let path = if path_arg.is_null() || path_arg == R_NilValue() {
            ".".to_string()
        } else {
            elt_to_string(path_arg, 0)
        };

        let entries: Vec<String> = std::fs::read_dir(&path)
            .map(|dir| {
                dir.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();

        let n = entries.len();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, name) in entries.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                crate::sexp::accessors::SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        result
    }
}

/// R's `system(command, intern = FALSE)` — run a system command.
pub unsafe fn do_system(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cmd = CAR(args);
        if cmd.is_null() {
            return R_NilValue();
        }
        let cmd_str = elt_to_string(cmd, 0);
        let intern = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            logical_arg(CAR(CDR(args)), false)
        } else {
            false
        };

        if system_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "system() is disabled by the session capability policy".to_string(),
            });
        }

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if intern {
                    let lines: Vec<&str> = stdout.lines().collect();
                    let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as R_xlen_t);
                    for (i, line) in lines.iter().enumerate() {
                        let cstr = CString::new(*line).unwrap_or_default();
                        SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                    result
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if !stdout.is_empty() {
                        crate::sexp::output::capture_stdout(&stdout);
                    }
                    if !stderr.is_empty() {
                        crate::sexp::output::capture_stderr(&stderr);
                    }
                    crate::sexp::globals::set_R_Visible(FALSE);
                    Rf_ScalarInteger(out.status.code().unwrap_or(1))
                }
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(127)
            }
        }
    }
}

/// R's `system2(command, args, stdout, stderr, wait, input)` — run a command.
pub unsafe fn do_system2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let command_arg = arg_by_name_or_position(args, &["command"], 0);
        if command_arg.is_null() || command_arg == R_NilValue() {
            return R_NilValue();
        }
        let command = elt_to_string(command_arg, 0);
        let argv_arg = arg_by_name_or_position(args, &["args"], 1);
        let argv = if argv_arg.is_null() || argv_arg == R_NilValue() {
            Vec::new()
        } else {
            (0..XLENGTH(argv_arg))
                .map(|i| elt_to_string(argv_arg, i))
                .filter(|arg| !arg.is_empty() && arg != "NA")
                .collect::<Vec<_>>()
        };
        let stdout_arg = arg_by_name_or_position(args, &["stdout"], 2);
        let capture_stdout = logical_arg(stdout_arg, false);

        if system_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "system2() is disabled by the session capability policy".to_string(),
            });
        }

        let output = std::process::Command::new(&command).args(&argv).output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if capture_stdout {
                    let lines = stdout.lines().map(str::to_string).collect::<Vec<_>>();
                    return string_vector(&lines);
                }
                if !stdout.is_empty() {
                    crate::sexp::output::capture_stdout(&stdout);
                }
                if !stderr.is_empty() {
                    crate::sexp::output::capture_stderr(&stderr);
                }
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(out.status.code().unwrap_or(1))
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(127)
            }
        }
    }
}

pub(crate) fn system_commands_disabled_by_runtime_policy() -> bool {
    !crate::sexp::instance::with_current_instance(|inst| unsafe {
        (*inst).eval_state.capabilities.allow_system_commands
    })
    .unwrap_or(false)
}

pub(crate) fn pipe_commands_disabled_by_runtime_policy() -> bool {
    !crate::sexp::instance::with_current_instance(|inst| unsafe {
        (*inst).eval_state.capabilities.allow_pipe_commands
    })
    .unwrap_or(false)
}

/// R's `stopifnot(...)` — stop if any condition is FALSE.
///
/// GNU `stopifnot(exprs = { ... })` evaluates each brace statement.
pub unsafe fn do_stopifnot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fail = |value: SEXP| {
            if value.is_null() || TYPEOF(value) != SEXPTYPE::LGLSXP {
                return;
            }
            let n = LENGTH(value);
            for i in 0..n {
                if *LOGICAL(value).add(i as usize) == 0 {
                    crate::mainutils::errors::errorcall_str(
                        crate::sexp::globals::R_NilValue(),
                        "FALSE is not TRUE",
                    );
                }
            }
        };

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let expr = CAR(current);
            let tag = TAG(current);
            let named_exprs = !tag.is_null()
                && tag != R_NilValue()
                && CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy() == "exprs";
            if named_exprs
                && TYPEOF(expr) == SEXPTYPE::LANGSXP
                && CAR(expr) == crate::sexp::symbol::R_BraceSymbol()
            {
                let _expr_guard = protect(expr);
                let mut stmt = CDR(expr);
                while !stmt.is_null() && stmt != R_NilValue() {
                    let _stmt_guard = protect(stmt);
                    fail(crate::eval::eval::Rf_eval(CAR(stmt), rho));
                    stmt = CDR(stmt);
                }
            } else if !named_exprs
                && !tag.is_null()
                && tag != R_NilValue()
                && matches!(
                    CStr::from_ptr(CHAR(PRINTNAME(tag)))
                        .to_string_lossy()
                        .as_ref(),
                    "exprObject" | "local"
                )
            {
                // GNU formals, not conditions.
            } else {
                fail(crate::eval::eval::Rf_eval(expr, rho));
            }
            current = CDR(current);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `nargs()` — number of arguments in the current call.
pub unsafe fn do_nargs(_call: SEXP, _op: SEXP, _args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // GNU util.c do_nargs: the first CTXT_FUNCTION context whose cloenv
        // is the calling environment supplies the promised-argument count;
        // callers under other frames (e.g. inside a builtin's promise
        // forcing) see their own closure, and no match yields NA.
        let mut cptr = crate::sexp::context::R_GlobalContext();
        while !cptr.is_null() {
            if (*cptr).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0
                && (*cptr).cloenv.as_raw() == rho
            {
                return Rf_ScalarInteger(pairlist_len((*cptr).promiseargs.as_raw()));
            }
            cptr = (*cptr).nextcontext;
        }
        Rf_ScalarInteger(NA_INTEGER as c_int)
    }
}
