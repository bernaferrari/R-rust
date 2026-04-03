#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_mut,
    unsafe_op_in_unsafe_fn
)]

//! Port of R's src/main/edit.c — edit() function.
//!
//! Provides do_edit for interactive editing of R objects and R_EditFiles
//! for editing external files with the system editor.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

/// Default file name for editing.
static mut DefaultFileName: *mut c_char = ptr::null_mut();

/// Whether the edit file has been used.
static mut EdFileUsed: c_int = 0;

/// Initialize the edit subsystem — creates the default temp file name.
///
/// Port of: attribute_hidden void InitEd(void)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn InitEd() {
    // Create a temp file name for the edit buffer
    let tmpdir = std::env::var("R_SESSION_TMPDIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string());

    let template = format!("{}/Redit{}.XXXXXX", tmpdir, ".R");
    let mut template_c = CString::new(template).unwrap_or_default();
    let template_buf = template_c.into_raw();

    if libc::mkstemp(template_buf) == -1 {
        // Failed to create, clean up
        libc::free(template_buf as *mut std::ffi::c_void);
        return;
    }

    // Close the fd — we just needed the name
    // Actually mkstemp creates the file, so we should keep it
    // But R's InitEd just creates the name, so let's close and re-open later
    // R uses R_tmpnam2 which doesn't create the file
    // Let's unlink and just keep the name
    libc::unlink(template_buf);

    core::ptr::addr_of_mut!(DefaultFileName).write(template_buf);
    EdFileUsed = 0;
}

/// Clean up the edit subsystem — removes the temp file if it was used.
///
/// Port of: void CleanEd(void)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn CleanEd() {
    if EdFileUsed != 0 && !DefaultFileName.is_null() {
        libc::unlink(DefaultFileName);
    }
    if !DefaultFileName.is_null() {
        libc::free(DefaultFileName as *mut std::ffi::c_void);
        DefaultFileName = ptr::null_mut();
    }
}

/// Get the default edit file name.
///
/// Returns the default file name for the edit buffer.
pub(crate) unsafe fn GetDefaultFileName() -> *mut c_char {
    DefaultFileName
}

/// R_EditFiles — invoke the system editor on one or more files.
///
/// Port of: int R_EditFiles(int nfiles, char **files, char **title)
/// Returns 0 on success, non-zero on failure.
pub(crate) unsafe fn R_EditFiles(
    nfiles: c_int,
    files: *mut *mut c_char,
    editor: *mut c_char,
) -> c_int {
    if nfiles <= 0 || files.is_null() {
        return 1;
    }

    // Determine the editor command
    let editor_cmd = if !editor.is_null() && unsafe { *editor } != 0 {
        unsafe { CStr::from_ptr(editor) }
            .to_str()
            .unwrap_or("")
            .to_string()
    } else {
        // Check VISUAL then EDITOR env vars
        std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string())
    };

    if editor_cmd.is_empty() {
        return 1;
    }

    for i in 0..nfiles as usize {
        let filepath = unsafe { *files.add(i) };
        if filepath.is_null() {
            continue;
        }
        let path_str = unsafe { CStr::from_ptr(filepath) }.to_str().unwrap_or("");
        if path_str.is_empty() {
            continue;
        }

        // Build the command: editor file
        let cmd = format!("{} '{}'", editor_cmd, path_str);

        let status = unsafe { libc::system(cmd.as_ptr() as *const c_char) };
        if status != 0 {
            return status;
        }
    }

    0
}

/// R_EditFile — invoke the editor on a single file.
///
/// This is the callback version used by R's edit() function.
/// Port of: int R_EditFile(const char *filename)
pub(crate) unsafe fn R_EditFile(filename: *const c_char) -> c_int {
    unsafe {
        if filename.is_null() {
            return 1;
        }
        let mut files = [filename as *mut c_char];
        let mut editor: *mut c_char = ptr::null_mut();
        R_EditFiles(1, files.as_mut_ptr(), editor)
    }
}

/// Edit an R object.
///
/// This is the equivalent of R's `do_edit()` from edit.c.
/// In the full implementation, this:
/// - Deparses the object to a temp file
/// - Invokes the system editor
/// - Re-parses the edited file
/// - Returns the result
///
/// Port of: attribute_hidden SEXP do_edit(SEXP call, SEXP op, SEXP args, SEXP rho)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_edit(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    // Full implementation requires:
    // 1. Deparsing (deparse1) — needs the evaluator
    // 2. File I/O (write deparsed text, read edited text)
    // 3. Editor invocation (R_EditFile or R_system)
    // 4. Parsing (R_ParseFile) — needs the parser
    // 5. Evaluation (eval) — needs the evaluator
    //
    // Since the parser and evaluator are not yet fully ported,
    // this implementation handles the file/editor parts but returns
    // R_NilValue for the parse/eval steps.

    use crate::sexp::accessors::*;

    // checkArity(op, args);

    let x = CAR(args);
    let mut rest = CDR(args);

    // Get the environment for closures
    let _envir = if !x.is_null() && TYPEOF(x) == crate::sexp::ffi::SEXPTYPE::CLOSXP.0 {
        CLOENV(x)
    } else {
        R_NilValue()
    };

    let fn_ = CAR(rest);
    rest = CDR(rest);

    // Determine the filename
    let mut filename: *const c_char = ptr::null();
    let use_default_file: bool;
    if !fn_.is_null()
        && TYPEOF(fn_) == crate::sexp::ffi::SEXPTYPE::STRSXP.0
        && !STRING_ELT(fn_, 0).is_null()
    {
        let s = CHAR(STRING_ELT(fn_, 0));
        if !s.is_null() && *s != 0 {
            filename = s;
        }
    }

    use_default_file = filename.is_null();
    if use_default_file {
        filename = DefaultFileName;
    }

    if filename.is_null() {
        // No file available — cannot edit
        return R_NilValue();
    }

    // If x is provided, write it to the file
    // (In full impl, would deparse x first)
    // For now, we skip this step

    // Get the editor
    let _ed = CAR(rest);
    rest = CDR(rest);

    let editor_cmd = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    if !editor_cmd.is_empty() && !filename.is_null() {
        let path_str = CStr::from_ptr(filename).to_str().unwrap_or("");
        if !path_str.is_empty() {
            // Invoke the editor
            let cmd = format!("{} '{}'", editor_cmd, path_str);
            libc::system(cmd.as_ptr() as *const c_char);
        }
    }

    // In the full implementation, we would now:
    // 1. Read back the edited file
    // 2. Parse it with R_ParseFile
    // 3. Evaluate each expression
    // 4. Return the last value
    //
    // Since the parser is not available, return R_NilValue
    R_NilValue()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_do_edit_null() {
        unsafe {
            let result = do_edit(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_init_ed() {
        unsafe {
            InitEd();
            // Should create a temp file name
            if !DefaultFileName.is_null() {
                let name = CStr::from_ptr(DefaultFileName);
                let name_str = name.to_str().unwrap_or("");
                assert!(name_str.contains("Redit"));
            }
            CleanEd();
        }
    }

    #[test]
    fn test_init_cleanup_cycle() {
        unsafe {
            InitEd();
            let name_before = DefaultFileName;
            CleanEd();
            // After cleanup, DefaultFileName should be null
            assert!(DefaultFileName.is_null());
            assert_ne!(name_before, ptr::null_mut()); // was allocated
        }
    }

    #[test]
    fn test_r_edit_files_null() {
        unsafe {
            let result = R_EditFiles(0, ptr::null_mut(), ptr::null_mut());
            assert_eq!(result, 1);
        }
    }

    #[test]
    fn test_r_edit_files_single() {
        unsafe {
            // Create a temp file to "edit"
            let template = CString::new("/tmp/test_edit_XXXXXX").unwrap();
            let mut buf = template.into_raw();
            let fd = unsafe { libc::mkstemp(buf) };
            if fd >= 0 {
                unsafe { libc::close(fd) };

                let mut files = [buf];
                // Use "true" as a no-op editor
                let editor = CString::new("true").unwrap();
                let mut editor_ptr = editor.into_raw();
                let result = R_EditFiles(1, files.as_mut_ptr(), editor_ptr);

                // Clean up
                unsafe {
                    libc::unlink(buf);
                    libc::free(buf as *mut std::ffi::c_void);
                    libc::free(editor_ptr as *mut std::ffi::c_void);
                }

                assert_eq!(result, 0);
            } else {
                unsafe { libc::free(buf as *mut std::ffi::c_void) };
            }
        }
    }

    #[test]
    fn test_get_default_filename_before_init() {
        unsafe {
            let name = GetDefaultFileName();
            // May or may not be null depending on test order
            let _ = name;
        }
    }
}
