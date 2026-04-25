#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Dialog functions for GraphApp.

use std::cell::RefCell;
use std::env;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::ptr;

use super::strings;
use super::types::*;

thread_local! {
    static USER_FILTER: RefCell<Option<CString>> = const { RefCell::new(None) };
    static LAST_MESSAGE: RefCell<Option<(c_int, String)>> = const { RefCell::new(None) };
}

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

unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
    }
}

fn make_graphapp_string(value: &str) -> *mut c_char {
    if let Ok(cstr) = CString::new(value) {
        unsafe { strings::new_string(cstr.as_ptr()) }
    } else {
        let filtered: String = value.chars().filter(|ch| *ch != '\0').collect();
        match CString::new(filtered) {
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

fn copy_into_buffer(result: &str, strbuf: *mut c_char, bufsize: c_int) -> *mut c_char {
    if strbuf.is_null() || bufsize <= 0 {
        return ptr::null_mut();
    }
    let bytes = result.as_bytes();
    let max_copy = (bufsize as usize).saturating_sub(1).min(bytes.len());
    unsafe {
        ptr::write_bytes(strbuf as *mut u8, 0, bufsize as usize);
        ptr::copy_nonoverlapping(bytes.as_ptr(), strbuf as *mut u8, max_copy);
        *strbuf.add(max_copy) = 0;
    }
    strbuf
}

fn remember_message(kind: c_int, text: &str) {
    LAST_MESSAGE.with(|slot| {
        *slot.borrow_mut() = Some((kind, text.to_owned()));
    });
}

fn write_status(obj: object, text: &str) {
    if obj.is_null() {
        return;
    }
    unsafe {
        (*obj).status.fill(0);
        for (idx, byte) in text
            .as_bytes()
            .iter()
            .copied()
            .take((*obj).status.len().saturating_sub(1))
            .enumerate()
        {
            (*obj).status[idx] = byte as c_char;
        }
    }
}

pub unsafe fn apperror(errstr: *const c_char) {
    let text = unsafe { cstr_to_string(errstr) };
    let message = if text.is_empty() {
        "Unspecified error".to_owned()
    } else {
        text
    };
    remember_message(-1, &message);
    eprintln!("graphapp error: {message}");
}

pub unsafe fn askok(info: *const c_char) {
    let text = unsafe { cstr_to_string(info) };
    remember_message(1, &text);
}

pub unsafe fn askokcancel(_question: *const c_char) -> c_int {
    dialog_choice(CANCEL).max(CANCEL)
}

pub unsafe fn askyesno(_question: *const c_char) -> c_int {
    match dialog_choice(NO) {
        YES => YES,
        _ => NO,
    }
}

pub unsafe fn askyesnocancel(_question: *const c_char) -> c_int {
    dialog_choice(CANCEL)
}

pub unsafe fn askstring(_question: *const c_char, default_string: *const c_char) -> *mut c_char {
    make_graphapp_string(&selected_text(unsafe { cstr_to_string(default_string) }))
}

pub unsafe fn askpassword(_question: *const c_char, default_string: *const c_char) -> *mut c_char {
    make_graphapp_string(&selected_text(unsafe { cstr_to_string(default_string) }))
}

pub unsafe fn askfilename(title: *const c_char, default_name: *const c_char) -> *mut c_char {
    unsafe { askfilenamewithdir(title, default_name, ptr::null()) }
}

pub unsafe fn askfilenamewithdir(
    _title: *const c_char,
    default_name: *const c_char,
    dir: *const c_char,
) -> *mut c_char {
    let value = select_path(unsafe { cstr_to_string(default_name) }, unsafe {
        cstr_to_string(dir)
    });
    if value.is_empty() {
        ptr::null_mut()
    } else {
        make_graphapp_string(&value)
    }
}

pub unsafe fn askfilesave(title: *const c_char, default_name: *const c_char) -> *mut c_char {
    unsafe { askfilesavewithdir(title, default_name, ptr::null()) }
}

pub unsafe fn askUserPass(_title: *const c_char) -> *mut c_char {
    make_graphapp_string(&selected_text(String::new()))
}

pub unsafe fn setuserfilter(filter: *const c_char) {
    USER_FILTER.with(|slot| {
        *slot.borrow_mut() = CString::new(unsafe { cstr_to_string(filter) }).ok();
    });
}

pub fn askchangedir() {
    if let Some(path) = env_dialog_input() {
        let _ = env::set_current_dir(path);
    }
}

pub unsafe fn askcdstring(_question: *const c_char, default_string: *const c_char) -> *mut c_char {
    make_graphapp_string(&selected_text(unsafe { cstr_to_string(default_string) }))
}

pub unsafe fn askfilesavewithdir(
    _title: *const c_char,
    default_name: *const c_char,
    dir: *const c_char,
) -> *mut c_char {
    let value = select_path(unsafe { cstr_to_string(default_name) }, unsafe {
        cstr_to_string(dir)
    });
    if value.is_empty() {
        ptr::null_mut()
    } else {
        make_graphapp_string(&value)
    }
}

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
    let value = select_path(unsafe { cstr_to_string(default_name) }, unsafe {
        cstr_to_string(dir)
    });
    copy_into_buffer(&value, strbuf, bufsize)
}

pub unsafe fn countFilenames(strbuf: *const c_char) -> c_int {
    if strbuf.is_null() {
        return 0;
    }
    let mut count = 0;
    let mut offset = 0usize;
    loop {
        let current = unsafe { strbuf.add(offset) };
        if unsafe { *current } == 0 {
            break;
        }
        count += 1;
        let len = unsafe { strings::string_length(current) as usize };
        offset += len + 1;
    }
    count
}

pub unsafe fn myMessageBox(obj: object, text: *const c_char, typ: c_int) {
    let message = unsafe { cstr_to_string(text) };
    remember_message(typ, &message);
    write_status(obj, &message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

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
        unsafe {
            let text = CString::new("hello").unwrap_or_else(|e| panic!("{e}"));
            myMessageBox(ptr::null_mut(), text.as_ptr(), 7);
            assert_eq!(
                LAST_MESSAGE.with(|slot| slot.borrow().clone()),
                Some((7, "hello".to_owned()))
            );
        }
    }
}
