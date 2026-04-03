#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Dialog functions for GraphApp.

use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn apperror(_errstr: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askok(_info: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askokcancel(_question: *const c_char) -> c_int {
    CANCEL
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askyesno(_question: *const c_char) -> c_int {
    CANCEL
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askyesnocancel(_question: *const c_char) -> c_int {
    CANCEL
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askstring(
    _question: *const c_char,
    _default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askpassword(
    _question: *const c_char,
    _default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilename(
    _title: *const c_char,
    _default_name: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilenamewithdir(
    _title: *const c_char,
    _default_name: *const c_char,
    _dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilesave(
    _title: *const c_char,
    _default_name: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askUserPass(_title: *const c_char) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setuserfilter(_filter: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askchangedir() { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askcdstring(
    _question: *const c_char,
    _default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilesavewithdir(
    _title: *const c_char,
    _default_name: *const c_char,
    _dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilenames(
    _title: *const c_char,
    _default_name: *const c_char,
    _multi: c_int,
    _filters: *const c_char,
    _filterindex: c_int,
    _strbuf: *mut c_char,
    _bufsize: c_int,
    _dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn countFilenames(_strbuf: *const c_char) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn myMessageBox(_obj: object, _text: *const c_char, _typ: c_int) {
    /* TODO */
}
