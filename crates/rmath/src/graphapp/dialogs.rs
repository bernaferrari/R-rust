#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Dialog functions for GraphApp.

use std::env;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::ptr;

use super::runtime::with_graphapp_runtime;
use super::strings;
use super::types::*;

fn env_dialog_value() -> Option<String> {
    env::var("RMATH_GRAPHAPP_DIALOG_RESPONSE")
        .ok()
        .or_else(|| env::var("GRAPHAPP_DIALOG_RESPONSE").ok())
}

fn dialog_choice(default: c_int) -> c_int {
    match env_dialog_value()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "yes" | "y" | "ok" | "true" | "1" => YES,
        "no" | "n" | "false" | "0" => NO,
        "cancel" | "c" => CANCEL,
        _ => default,
    }
}

fn env_dialog_input() -> Option<String> {
    env::var("RMATH_GRAPHAPP_DIALOG_INPUT")
        .ok()
        .or_else(|| env::var("GRAPHAPP_DIALOG_INPUT").ok())
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    // SAFETY: forwarded raw pointers satisfy `cstr`'s contract per this function's own `# Safety` requirements.
    (unsafe { cstr(ptr) }).map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

fn make_graphapp_string(value: &str) -> *mut c_char {
    if let Ok(cstr) = CString::new(value) {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        unsafe { strings::new_string(cstr.as_ptr()) }
    } else {
        let filtered: String = value.chars().filter(|ch| *ch != '\0').collect();
        match CString::new(filtered) {
            // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
            Ok(cstr) => unsafe { strings::new_string(cstr.as_ptr()) },
            Err(_) => ptr::null_mut(),
        }
    }
}

fn selected_text(default_value: String) -> String {
    env_dialog_input().unwrap_or(default_value)
}

fn select_path(default_name: String, dir: String) -> String {
    if let Some(explicit) = env_dialog_input() {
        return explicit;
    }
    if default_name.is_empty() {
        return dir;
    }
    let path = Path::new(&default_name);
    if dir.is_empty() || path.is_absolute() {
        default_name
    } else {
        PathBuf::from(dir).join(path).to_string_lossy().into_owned()
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn copy_into_buffer(result: &str, strbuf: *mut c_char, bufsize: c_int) -> *mut c_char {
    if strbuf.is_null() || bufsize <= 0 {
        return ptr::null_mut();
    }
    let bytes = result.as_bytes();
    let max_copy = (bufsize as usize).saturating_sub(1).min(bytes.len());
    // Zero and fill the caller's `bufsize`-byte buffer.
    // SAFETY: forwarded raw pointers satisfy `slice_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(buf) = unsafe { slice_mut(strbuf as *mut u8, bufsize as usize) } {
        buf.fill(0);
        buf[..max_copy].copy_from_slice(&bytes[..max_copy]);
        buf[max_copy] = 0;
    }
    strbuf
}

fn remember_message(kind: c_int, text: &str) {
    with_graphapp_runtime(|runtime| runtime.dialogs.last_message = Some((kind, text.to_owned())));
}

fn change_dir_if_allowed(path: &Path) -> bool {
    let allowed = crate::sexp::instance::with_required_current_instance(|inst| {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        (unsafe { ptr_mut(inst) })
            .is_some_and(|i| i.eval_state.capabilities.allow_environment_mutation)
    });
    allowed && env::set_current_dir(path).is_ok()
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn write_status(obj: object, text: &str) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { obj_mut(obj) }) else {
        return;
    };
    data.status.fill(0);
    for (idx, byte) in text
        .as_bytes()
        .iter()
        .copied()
        .take(data.status.len().saturating_sub(1))
        .enumerate()
    {
        data.status[idx] = byte as c_char;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn apperror(errstr: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let text = unsafe { cstr_to_string(errstr) };
    let message = if text.is_empty() {
        "Unspecified error".to_owned()
    } else {
        text
    };
    remember_message(-1, &message);
    eprintln!("graphapp error: {message}");
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askok(info: *const c_char) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let text = unsafe { cstr_to_string(info) };
    remember_message(1, &text);
}

pub fn askokcancel(_question: *const c_char) -> c_int {
    dialog_choice(CANCEL).max(CANCEL)
}

pub fn askyesno(_question: *const c_char) -> c_int {
    match dialog_choice(NO) {
        YES => YES,
        _ => NO,
    }
}

pub fn askyesnocancel(_question: *const c_char) -> c_int {
    dialog_choice(CANCEL)
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askstring(_question: *const c_char, default_string: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    make_graphapp_string(&selected_text(unsafe { cstr_to_string(default_string) }))
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askpassword(_question: *const c_char, default_string: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    make_graphapp_string(&selected_text(unsafe { cstr_to_string(default_string) }))
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askfilename(title: *const c_char, default_name: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `askfilenamewithdir`'s contract per this function's own `# Safety` requirements.
    unsafe { askfilenamewithdir(title, default_name, ptr::null()) }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askfilenamewithdir(
    _title: *const c_char,
    default_name: *const c_char,
    dir: *const c_char,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let value = select_path(unsafe { cstr_to_string(default_name) }, unsafe {
        cstr_to_string(dir)
    });
    if value.is_empty() {
        ptr::null_mut()
    } else {
        make_graphapp_string(&value)
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askfilesave(title: *const c_char, default_name: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `askfilesavewithdir`'s contract per this function's own `# Safety` requirements.
    unsafe { askfilesavewithdir(title, default_name, ptr::null()) }
}

pub fn askUserPass(_title: *const c_char) -> *mut c_char {
    make_graphapp_string(&selected_text(String::new()))
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn setuserfilter(filter: *const c_char) {
    with_graphapp_runtime(|runtime| {
        // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
        runtime.dialogs.user_filter = CString::new(unsafe { cstr_to_string(filter) }).ok();
    });
}

pub fn askchangedir() {
    if let Some(path) = env_dialog_input() {
        let _ = change_dir_if_allowed(Path::new(&path));
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askcdstring(_question: *const c_char, default_string: *const c_char) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    make_graphapp_string(&selected_text(unsafe { cstr_to_string(default_string) }))
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askfilesavewithdir(
    _title: *const c_char,
    default_name: *const c_char,
    dir: *const c_char,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let value = select_path(unsafe { cstr_to_string(default_name) }, unsafe {
        cstr_to_string(dir)
    });
    if value.is_empty() {
        ptr::null_mut()
    } else {
        make_graphapp_string(&value)
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn askfilenames(
    _title: *const c_char,
    default_name: *const c_char,
    _multi: c_int,
    _filters: *const c_char,
    _filterindex: c_int,
    strbuf: *mut c_char,
    bufsize: c_int,
    dir: *const c_char,
) -> *mut c_char {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let value = select_path(unsafe { cstr_to_string(default_name) }, unsafe {
        cstr_to_string(dir)
    });
    // SAFETY: forwarded raw pointers satisfy `copy_into_buffer`'s contract per this function's own `# Safety` requirements.
    unsafe { copy_into_buffer(&value, strbuf, bufsize) }
}

pub fn countFilenames(strbuf: *const c_char) -> c_int {
    if strbuf.is_null() {
        return 0;
    }
    let mut count = 0;
    let mut offset = 0usize;
    loop {
        // Step through the caller's multi-NUL-terminated buffer.
        let current = strbuf.wrapping_add(offset);
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        let Some(segment) = (unsafe { cstr(current) }) else {
            break;
        };
        if segment.to_bytes().is_empty() {
            break;
        }
        count += 1;
        offset += segment.to_bytes().len() + 1;
    }
    count
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn myMessageBox(obj: object, text: *const c_char, typ: c_int) {
    // SAFETY: forwarded raw pointers satisfy `cstr_to_string`'s contract per this function's own `# Safety` requirements.
    let message = unsafe { cstr_to_string(text) };
    remember_message(typ, &message);
    // SAFETY: forwarded raw pointers satisfy `write_status`'s contract per this function's own `# Safety` requirements.
    unsafe { write_status(obj, &message) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    #[test]
    fn ask_change_directory_is_denied_without_host_mutation_capability() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        let before = env::current_dir().expect("current directory");
        let changed = change_dir_if_allowed(Path::new("/"));
        let after = env::current_dir().expect("current directory");
        if before != after {
            env::set_current_dir(&before).expect("restore directory after regression");
        }
        assert!(!changed);
        assert_eq!(after, before);
    }

    #[test]
    fn filename_helpers_join_directory_without_prompting() {
        let title = CString::new("Open").unwrap_or_else(|e| panic!("{e}"));
        let name = CString::new("report.txt").unwrap_or_else(|e| panic!("{e}"));
        let dir = CString::new("/tmp").unwrap_or_else(|e| panic!("{e}"));

        unsafe {
            let value = askfilenamewithdir(title.as_ptr(), name.as_ptr(), dir.as_ptr());
            let rendered = CStr::from_ptr(value).to_string_lossy().into_owned();
            assert!(rendered.ends_with("/tmp/report.txt"));
        }
    }

    #[test]
    fn askfilenames_populates_buffer_and_count_matches_segments() {
        let default_name = CString::new("file.csv").unwrap_or_else(|e| panic!("{e}"));
        let mut buffer = [0i8; 64];

        unsafe {
            let result = askfilenames(
                ptr::null(),
                default_name.as_ptr(),
                0,
                ptr::null(),
                0,
                buffer.as_mut_ptr(),
                buffer.len() as c_int,
                ptr::null(),
            );
            assert_eq!(countFilenames(result), 1);
            assert_eq!(CStr::from_ptr(result).to_bytes(), b"file.csv");
        }
    }

    #[test]
    fn message_box_records_status_text() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();

        let text = CString::new("hello").unwrap_or_else(|e| panic!("{e}"));
        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        unsafe { myMessageBox(ptr::null_mut(), text.as_ptr(), 7) };
        assert_eq!(
            with_graphapp_runtime(|runtime| runtime.dialogs.last_message.clone()),
            Some((7, "hello".to_owned()))
        );
    }
}
