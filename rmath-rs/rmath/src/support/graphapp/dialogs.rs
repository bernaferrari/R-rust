#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Dialog functions for GraphApp.

use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

pub unsafe fn apperror(_errstr: *const c_char) { /* TODO */
}
pub unsafe fn askok(_info: *const c_char) { /* TODO */
}
pub unsafe fn askokcancel(_question: *const c_char) -> c_int {
    CANCEL
}
pub unsafe fn askyesno(_question: *const c_char) -> c_int {
    CANCEL
}
pub unsafe fn askyesnocancel(_question: *const c_char) -> c_int {
    CANCEL
}
pub unsafe fn askstring(
    _question: *const c_char,
    _default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askpassword(
    _question: *const c_char,
    _default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askfilename(
    _title: *const c_char,
    _default_name: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askfilenamewithdir(
    _title: *const c_char,
    _default_name: *const c_char,
    _dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askfilesave(
    _title: *const c_char,
    _default_name: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askUserPass(_title: *const c_char) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn setuserfilter(_filter: *const c_char) { /* TODO */
}
pub unsafe fn askchangedir() { /* TODO */
}
pub unsafe fn askcdstring(
    _question: *const c_char,
    _default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askfilesavewithdir(
    _title: *const c_char,
    _default_name: *const c_char,
    _dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
pub unsafe fn askfilenames(
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
pub unsafe fn countFilenames(_strbuf: *const c_char) -> c_int {
    0
}
pub unsafe fn myMessageBox(_obj: object, _text: *const c_char, _typ: c_int) {
    /* TODO */
}
