#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Dialog functions for GraphApp.

use std::os::raw::{c_char, c_int};
use std::ptr;

use super::types::*;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn apperror(errstr: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askok(info: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askokcancel(question: *const c_char) -> c_int {
    CANCEL
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askyesno(question: *const c_char) -> c_int {
    CANCEL
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askyesnocancel(question: *const c_char) -> c_int {
    CANCEL
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askstring(
    question: *const c_char,
    default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askpassword(
    question: *const c_char,
    default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilename(
    title: *const c_char,
    default_name: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilenamewithdir(
    title: *const c_char,
    default_name: *const c_char,
    dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilesave(
    title: *const c_char,
    default_name: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askUserPass(title: *const c_char) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setuserfilter(filter: *const c_char) { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askchangedir() { /* TODO */
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askcdstring(
    question: *const c_char,
    default_string: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilesavewithdir(
    title: *const c_char,
    default_name: *const c_char,
    dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn askfilenames(
    title: *const c_char,
    default_name: *const c_char,
    multi: c_int,
    filters: *const c_char,
    filterindex: c_int,
    strbuf: *mut c_char,
    bufsize: c_int,
    dir: *const c_char,
) -> *mut c_char {
    ptr::null_mut()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn countFilenames(strbuf: *const c_char) -> c_int {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn myMessageBox(obj: object, text: *const c_char, typ: c_int) {
    /* TODO */
}
